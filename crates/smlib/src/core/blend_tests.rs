//! Fillets and chamfers on solidcore, against closed forms: every result exports to a record that
//! validates. With SMLib built, the same case on the C++ kernel gives the same volume, counts and
//! faces by surface kind, or the same error (parity only: the expected values are the analytic
//! ones).
//!
//! The bodies are solidcore's own boxes and cylinders.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::mpsc;
use std::time::Duration;

use super::blend::{Corners, blend, blend_dropping, blend_holes, chamfer_sides, fillet_variable};
use super::boolean::boolean;
use super::{Body, counts, make_box, make_cylinder, to_arrays, volume};
use crate::export::native;
use crate::{Blend, BooleanOp, ChamferSecond, Error};

/// A case longer than this is a hang.
const TIMEOUT: Duration = Duration::from_secs(300);

/// How near a pick point must be.
const TOL: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Box([f64; 3], [f64; 3]),
    Cylinder([f64; 3], f64, f64),
    /// A box (corner, size) less a cylinder along z (base centre, radius, height).
    Holed([f64; 3], [f64; 3], [f64; 3], f64, f64),
}

impl Shape {
    fn core(self) -> Body {
        match self {
            Shape::Box(o, s) => make_box(o, s),
            Shape::Cylinder(o, r, h) => make_cylinder(o, r, h),
            Shape::Holed(o, s, c, r, h) => {
                boolean(&make_box(o, s).unwrap(), &make_cylinder(c, r, h).unwrap(), BooleanOp::Subtract).map(|b| b.unwrap())
            }
        }
        .unwrap()
    }
}

#[derive(Clone, Debug)]
enum Op {
    /// `Brep::blend` of one kind, one size for every edge.
    Blend(Shape, Blend, Vec<[f64; 3]>, f64),
    /// `Brep::chamfer_sides`.
    Sides(Shape, Vec<[f64; 3]>, f64, ChamferSecond, [f64; 3]),
    /// `Brep::blend_setback` of rounds: edges, radius, corners, setbacks.
    Setback(Shape, Vec<[f64; 3]>, f64, Vec<[f64; 3]>, Vec<f64>),
    /// `Brep::fillet_variable`: edge point, start, degree, knots, radii.
    Variable(Shape, [f64; 3], [f64; 3], usize, Vec<f64>, Vec<f64>),
}

/// What a case must give.
#[derive(Clone, Debug)]
enum Expect {
    /// A body of this volume (relative 1e-9).
    Volume(f64),
    /// A body of this volume to this relative accuracy.
    Near(f64, f64),
    /// A body whose volume lies strictly between these.
    Between(f64, f64),
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
/// Its top front edge, along x at y = 0, z = 10, 30 long.
const TOP_FRONT: [f64; 3] = [15.0, 0.0, 10.0];

/// The cross-section a round of radius r takes from a right-angled edge: (1 - π/4) r².
fn spandrel(r: f64) -> f64 {
    (1.0 - PI / 4.0) * r * r
}

fn case(name: &str) -> Case {
    let c = |op, expect| Case { op, expect };
    let r = 2.0;
    match name {
        // abc - (1 - π/4) r² L.
        "fillet_one_edge" => c(Op::Blend(BLOCK, Blend::Round, vec![TOP_FRONT], r), Expect::Volume(ABC - spandrel(r) * 30.0)),
        // Every edge rounded at r, the corners spheres: (a-2r)(b-2r)(c-2r) + 2r Σ(faces less
        // 2r) + πr² Σ(edges less 2r) + 4/3 πr³.
        "fillet_all_edges" => {
            let (a, b, h) = (30.0 - 2.0 * r, 20.0 - 2.0 * r, 10.0 - 2.0 * r);
            let v = a * b * h + 2.0 * r * (a * b + b * h + h * a) + PI * r * r * (a + b + h) + 4.0 / 3.0 * PI * r * r * r;
            let mut edges = Vec::new();
            for (y, z) in [(0.0, 0.0), (20.0, 0.0), (0.0, 10.0), (20.0, 10.0)] {
                edges.push([15.0, y, z]);
            }
            for (x, z) in [(0.0, 0.0), (30.0, 0.0), (0.0, 10.0), (30.0, 10.0)] {
                edges.push([x, 10.0, z]);
            }
            for (x, y) in [(0.0, 0.0), (30.0, 0.0), (0.0, 20.0), (30.0, 20.0)] {
                edges.push([x, y, 5.0]);
            }
            c(Op::Blend(BLOCK, Blend::Round, edges, r), Expect::Volume(v))
        }
        // Each section of a curvature-continuous round is the quintic from contact to contact
        // whose inner control points lie a third and two thirds of the way to the corner the
        // contacts' tangents make (fork fd3b6a7). On the unit square's sides it takes
        // ½∮(x dy - y dx) = 113/756 from the corner: abc - 113/756 r² L.
        "fillet_curvature_one_edge" => c(Op::Blend(BLOCK, Blend::Curvature, vec![TOP_FRONT], r), Expect::Volume(ABC - 113.0 / 756.0 * r * r * 30.0)),
        // abc - d² L / 2.
        "chamfer_one_edge" => c(Op::Blend(BLOCK, Blend::Chamfer, vec![TOP_FRONT], r), Expect::Volume(ABC - r * r * 30.0 / 2.0)),
        // abc - d1 d2 L / 2: 2 back along the top (the face leaning most along +z), 3 down the
        // front.
        "chamfer_two_distances" => {
            c(Op::Sides(BLOCK, vec![TOP_FRONT], 2.0, ChamferSecond::Distance(3.0), [0.0, 0.0, 1.0]), Expect::Volume(ABC - 2.0 * 3.0 * 30.0 / 2.0))
        }
        // 2 back along the top, at 30° to it: d tan 30° down the front.
        "chamfer_distance_angle" => c(
            Op::Sides(BLOCK, vec![TOP_FRONT], 2.0, ChamferSecond::Angle(PI / 6.0), [0.0, 0.0, 1.0]),
            Expect::Volume(ABC - 2.0 * 2.0 * (PI / 6.0).tan() * 30.0 / 2.0),
        ),
        // A 5 mm cylinder 10 high, its top rim rounded at 1. Pappus: the spandrel (1 - π/4) r²,
        // its centroid (10 - 3π)/(12 - 3π) r in from the rim, swept round the axis.
        "fillet_cylinder_rim" => {
            let (big, h, r) = (5.0, 10.0, 1.0);
            let inset = (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI) * r;
            c(
                Op::Blend(Shape::Cylinder([0.0; 3], big, h), Blend::Round, vec![[0.0, big, h]], r),
                Expect::Volume(PI * big * big * h - 2.0 * PI * (big - inset) * spandrel(r)),
            )
        }
        // The three edges at the corner (30, 20, 10) rounded at r, the corner set back 4 along
        // each. Without a setback the corner is a sphere's octant, and the block keeps
        // abc - (1 - π/4) r² Σ(L - r) - (1 - π/6) r³; the setback changes only what lies within
        // s + r of the corner, a cube of side s + r.
        "setback_corner" => {
            let s = 4.0;
            let plain = ABC - spandrel(r) * (30.0 + 20.0 + 10.0 - 3.0 * r) - (1.0 - PI / 6.0) * r * r * r;
            let near = (s + r) * (s + r) * (s + r);
            c(
                Op::Setback(BLOCK, vec![[15.0, 20.0, 10.0], [30.0, 10.0, 10.0], [30.0, 20.0, 5.0]], r, vec![[30.0, 20.0, 10.0]], vec![s]),
                Expect::Between(plain - near, plain + near),
            )
        }
        // The top front edge rounded from 1 at x = 0 to 3 at x = 30, linearly, each section
        // square to the edge a quarter circle: abc - (1 - π/4) ∫ r(x)² dx
        // = abc - (1 - π/4) L (r0² + r0 r1 + r1²) / 3. The round is a B-spline surface fitted to
        // those sections, which both kernels build 2.5e-7 short of it (and within 1e-9 of each
        // other), within the solver's fitting tolerance.
        "fillet_variable_linear" => {
            let (r0, r1, l) = (1.0, 3.0, 30.0);
            c(
                Op::Variable(BLOCK, TOP_FRONT, [0.0, 0.0, 10.0], 1, vec![0.0, 0.0, 1.0, 1.0], vec![r0, r1]),
                Expect::Near(ABC - (1.0 - PI / 4.0) * l * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0, 1e-6),
            )
        }
        // A round of 12 on a 10 mm cube's top front edge is larger than either face beside it:
        // the fork builds it over the cliffs, on the faces' continuations, and cuts away all of
        // the cube outside the round's cylinder (axis y = 12, z = -2, radius 12). What stays is
        // 10 times the square's part inside that circle, ∫ (√(144 - u²) - 2) du over
        // u = y - 12 from -√140 to -2: 72 (asin(√140/12) - asin(1/6)) - 2 (√140 - 2).
        "fillet_over_a_cliff" => {
            let w = 140f64.sqrt();
            let area = 72.0 * ((w / 12.0).asin() - (1.0f64 / 6.0).asin()) - 2.0 * (w - 2.0);
            c(Op::Blend(Shape::Box([0.0; 3], [10.0; 3]), Blend::Round, vec![[5.0, 0.0, 10.0]], 12.0), Expect::Volume(10.0 * area))
        }
        // A round of 6 on the top rim of a cylinder of radius 5 is wider than the cylinder.
        "fillet_too_large" => {
            c(Op::Blend(Shape::Cylinder([0.0; 3], 5.0, 10.0), Blend::Round, vec![[0.0, 5.0, 10.0]], 6.0), Expect::Fails(Error::NoRoom))
        }
        "pick_near_nothing" => {
            c(Op::Blend(BLOCK, Blend::Round, vec![TOP_FRONT, [100.0, 100.0, 100.0]], r), Expect::Fails(Error::NoMatch { point: 1 }))
        }
        // A 20 mm cube with a hole of radius 5 along z whose wall touches the face y = 0 along
        // x = 10 (#1390): a 3 mm chamfer of the hole's top rim runs out of that face where the
        // rim meets it tangentially, a vertex no blend corner handles. The tangent chain the app
        // blends is picked: the rim at 135° and 315° round the hole (away from the cylinder's
        // seam at 0° and the face at 270°), and the two pieces of the top's front edge, which the
        // rim runs on tangentially into at (10, 0, 20).
        "chamfer_hole_rim_touching_a_face" => {
            let d = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
            c(
                Op::Blend(
                    Shape::Holed([0.0; 3], [20.0; 3], [10.0, 5.0, -1.0], 5.0, 22.0),
                    Blend::Chamfer,
                    vec![[10.0 - d, 5.0 + d, 20.0], [10.0 + d, 5.0 - d, 20.0], [5.0, 0.0, 20.0], [15.0, 0.0, 20.0]],
                    3.0,
                ),
                Expect::Fails(Error::CornerUnsupported),
            )
        }
        // √3 from all three edges at the corner (0, 0, 10).
        "pick_ambiguous" => c(Op::Blend(BLOCK, Blend::Round, vec![[-1.0, -1.0, 11.0]], r), Expect::Fails(Error::Ambiguous { point: 0 })),
        _ => panic!("{name}"),
    }
}

/// The tolerance a case picks with: the ambiguous point lies √3 from its edges.
fn tolerance(name: &str) -> f64 {
    if name == "pick_ambiguous" { 5.0 } else { TOL }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

#[derive(Debug, PartialEq)]
struct Measured {
    volume: f64,
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

fn on_core(name: &str, c: &Case) -> crate::Result<Body> {
    let tol = tolerance(name);
    match &c.op {
        Op::Blend(s, kind, points, size) => blend(&s.core(), *kind, points, &[*size], tol, &[], &[], Corners::Filled),
        Op::Sides(s, points, d, second, first) => chamfer_sides(&s.core(), points, tol, *d, *second, *first),
        Op::Setback(s, points, size, corners, setbacks) => {
            blend(&s.core(), Blend::Round, points, &vec![*size; points.len()], tol, corners, setbacks, Corners::Filled)
        }
        Op::Variable(s, point, start, degree, knots, radii) => fillet_variable(&s.core(), *point, tol, *start, *degree, knots, radii),
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

/// Checks a kernel's result against the case's closed form; what it measured, or its error.
fn judge(name: &str, who: &str, expect: &Expect, got: Got) -> Got {
    match (expect, &got) {
        (Expect::Fails(e), Err(g)) => assert_eq!(g, e, "{name}: {who} failed otherwise"),
        (Expect::Fails(e), Ok(m)) => panic!("{name}: {who} gave {m:?}, expected {e:?}"),
        (_, Err(g)) => panic!("{name}: {who} failed: {g}"),
        (Expect::Volume(v), Ok(m)) => assert!(rel(m.volume, *v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.volume),
        (Expect::Near(v, within), Ok(m)) => assert!(rel(m.volume, *v) < *within, "{name}: {who} volume {} vs analytic {v}", m.volume),
        (Expect::Between(lo, hi), Ok(m)) => assert!(*lo < m.volume && m.volume < *hi, "{name}: {who} volume {} not in ({lo}, {hi})", m.volume),
    }
    got
}

fn solidcore_case(name: &'static str) -> Got {
    timed(name, move || {
        let c = case(name);
        let got = on_core(name, &c).map(|body| {
            let v = volume(&body, 1e-8).unwrap_or_else(|e| panic!("{name}: volume: {e}"));
            let n = counts(&body).unwrap();
            let (rec, _) = native(&to_arrays(&body).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
            rec.validate().unwrap_or_else(|e| panic!("{name}: record: {e}"));
            Measured { volume: v, counts: n, kinds: kinds(&rec) }
        });
        judge(name, "solidcore", &c.expect, got)
    })
}

/// The case on the C++ kernel, called directly (whatever the routing).
#[cfg(feature = "native")]
fn cpp_case(name: &str) -> Got {
    use crate::ops::{pick_error, setback_error};
    use crate::{Brep, ffi};
    crate::init().unwrap();
    let make = |s: Shape| -> Brep {
        match s {
            Shape::Box(o, z) => ffi::make_box(&o, &z),
            Shape::Cylinder(o, r, h) => ffi::make_cylinder(&o, r, h),
            Shape::Holed(o, z, c, r, h) => {
                ffi::ops::boolean(&ffi::make_box(&o, &z).unwrap(), &ffi::make_cylinder(&c, r, h).unwrap(), ffi::ops::BOOLEAN_SUBTRACT)
            }
        }
        .map(Brep::made)
        .unwrap()
    };
    let kind = |k| match k {
        Blend::Round => ffi::ops::BLEND_ROUND,
        Blend::Curvature => ffi::ops::BLEND_CURVATURE,
        Blend::Chord => ffi::ops::BLEND_CHORD,
        Blend::Chamfer => ffi::ops::BLEND_CHAMFER,
    };
    let c = case(name);
    let tol = tolerance(name);
    let r = match &c.op {
        Op::Blend(s, k, points, size) => {
            ffi::ops::blend(kind(*k), make(*s).cpp().unwrap(), points, &vec![*size; points.len()], tol).map_err(pick_error)
        }
        Op::Sides(s, points, d, second, first) => {
            let (value, angle) = match second {
                ChamferSecond::Distance(d) => (*d, false),
                ChamferSecond::Angle(a) => (*a, true),
            };
            ffi::ops::chamfer_sides(make(*s).cpp().unwrap(), points, tol, *d, value, angle, first).map_err(pick_error)
        }
        Op::Setback(s, points, size, corners, setbacks) => {
            ffi::ops::blend_setback(ffi::ops::BLEND_ROUND, make(*s).cpp().unwrap(), points, &vec![*size; points.len()], corners, setbacks, tol)
                .map_err(setback_error)
        }
        Op::Variable(s, point, start, degree, knots, radii) => {
            ffi::ops::fillet_variable(make(*s).cpp().unwrap(), point, tol, start, i32::try_from(*degree).unwrap(), knots, radii).map_err(
                |s| match s {
                    ffi::ops::ERR_PICK_NONE => Error::NoMatch { point: 0 },
                    ffi::ops::ERR_PICK_AMBIGUOUS => Error::Ambiguous { point: 0 },
                    s => crate::error(s),
                },
            )
        }
    };
    let got = r.map(|h| {
        let v = ffi::volume(&h, 1e-8).unwrap();
        let n = ffi::counts(&h).unwrap().map(|x| usize::try_from(x).unwrap());
        let b = Brep::made(h);
        let (rec, _) = native(&b.cpp_arrays().unwrap()).unwrap();
        rec.validate().unwrap();
        Measured { volume: v, counts: n, kinds: kinds(&rec) }
    });
    judge(name, "C++", &c.expect, got)
}

/// solidcore and C++ agree: volume within 1e-9, the same counts and faces by kind, or the same
/// error.
#[cfg(feature = "native")]
fn parity(name: &'static str) {
    let cpp = cpp_case(name);
    let ours = solidcore_case(name);
    match (ours, cpp) {
        (Ok(a), Ok(b)) => {
            assert!(rel(a.volume, b.volume) < 1e-9, "{name}: solidcore {} vs C++ {}", a.volume, b.volume);
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
    fillet_one_edge,
    fillet_all_edges,
    fillet_curvature_one_edge,
    chamfer_one_edge,
    chamfer_two_distances,
    chamfer_distance_angle,
    fillet_cylinder_rim,
    setback_corner,
    fillet_variable_linear,
    fillet_over_a_cliff,
    fillet_too_large,
    chamfer_hole_rim_touching_a_face,
    pick_near_nothing,
    pick_ambiguous,
);

/// What solidcore does not have yet is refused as not supported, after the capi's own checks.
#[test]
fn fork_only_blends_are_not_supported_yet() {
    let r = timed("unsupported", || {
        let b = BLOCK.core();
        // 1 to 2 and back to 1: a corner at the middle.
        let bent = fillet_variable(&b, TOP_FRONT, TOL, [0.0, 0.0, 10.0], 1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 2.0, 1.0]).err();
        // A pick that fails still says so first.
        let far = chamfer_sides(&b, &[[100.0; 3]], TOL, 2.0, ChamferSecond::Distance(3.0), [0.0, 0.0, 1.0]).err();
        (bent, far)
    });
    assert_eq!(r.0, Some(Error::Unsupported));
    assert_eq!(r.1, Some(Error::NoMatch { point: 0 }));
}

/// A cliff overflow the fork refuses too (`fixes.fillet_cliff_overflow`, solidcore#960): the
/// three edges at a 20 mm cube's corner rounded r20, their fillet's boundary on two faces'
/// surfaces, has no room, as on the fork, not "not supported".
#[test]
fn a_cliff_overflow_the_fork_refuses_has_no_room() {
    let r = timed("cliff_refused", || {
        let corner = [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]];
        blend(&Shape::Box([0.0; 3], [20.0; 3]).core(), Blend::Round, &corner, &[20.0; 3], TOL, &[], &[], Corners::Filled).err()
    });
    assert_eq!(r, Some(Error::NoRoom));
}

/// The capi's `overflow_goes_first` (fork 241f7ba): R21 on a 20 cube's edge takes all of both faces
/// that edge lies on, and rounds with it on other edges go after it. With R5 on the corner's other
/// two edges those edges are gone (the two R5 points flagged) and the result is the R21's alone,
/// 20 (F(w) - F(1) - (w - 1)) with F the integral of sqrt(441 - u²) and w = sqrt(440). With R5 on
/// the back upright that upright's round runs above the R21 cylinder, taking the corner's piece
/// over it too (Simpson on 4000 spans, against a bound of its quartic end edge's tolerance).
#[test]
#[ignore = "core::blend::OVERFLOW_GOES_FIRST is held back: solidcore's cliff overflow loses edge attributes (an_edge_tag_crosses_a_cliff_overflow). Passes with it on: 6107.529369565"]
fn overflow_goes_first() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    let f = |u: f64| 0.5 * (u * (441.0 - u * u).sqrt() + 441.0 * (u / 21.0).asin());
    let w = 440f64.sqrt();
    let alone = 20.0 * (f(w) - f(1.0) - (w - 1.0));
    let strip = |t: f64| {
        let y = 15.0 + 5.0 * t.sin();
        let zc = 21.0 - (441.0 - (y - 21.0) * (y - 21.0)).sqrt();
        5.0 * (1.0 - t.cos()) * (20.0 - zc) * 5.0 * t.cos()
    };
    let (n, h) = (4000, 0.5 * PI / 4000.0);
    let mut sum = strip(0.0) + strip(0.5 * PI);
    for i in 1..n {
        sum += f64::from(if i % 2 == 1 { 4 } else { 2 }) * strip(f64::from(i) * h);
    }
    let corner = sum * h / 3.0;
    assert!((corner - 106.612_300_588).abs() < 1e-6, "{corner}");
    let cases: [(&[[f64; 3]], &[f64], &[bool], f64, f64); 2] = [
        (&[[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]], &[21.0, 5.0, 5.0], &[false, true, true], alone, 1e-9 * alone),
        (&[[10.0, 0.0, 0.0], [0.0, 20.0, 10.0]], &[21.0, 5.0], &[false, false], alone - corner, 0.02),
    ];
    for (points, sizes, dropped, want, bound) in cases {
        let got = timed("overflow_first", move || {
            let (b, d) = blend_dropping(&Shape::Box([0.0; 3], [20.0; 3]).core(), Blend::Round, points, sizes, 1.0).unwrap();
            native(&to_arrays(&b).unwrap()).unwrap();
            (volume(&b, 1e-8).unwrap(), d)
        });
        println!("overflow first {sizes:?}: {:.9}, expected {want:.9}", got.0);
        assert_eq!(got.1, dropped, "{sizes:?}");
        assert!((got.0 - want).abs() <= bound, "{sizes:?}: {} vs {want}", got.0);
        #[cfg(feature = "native")]
        {
            let body = crate::Brep::made_core(Shape::Box([0.0; 3], [20.0; 3]).core());
            let (cpp, d) = crate::ffi::ops::blend_dropping(crate::ffi::ops::BLEND_ROUND, body.cpp().unwrap(), points, sizes, 1.0).unwrap();
            assert_eq!(d, dropped, "C++ {sizes:?}");
            let v = crate::ffi::volume(&cpp, 1e-8).unwrap();
            assert!((got.0 - v).abs() <= bound, "{sizes:?}: solidcore {} vs C++ {v}", got.0);
        }
    }
}

/// The capi's `rounds_at_a_concave_face_corner_are_mitred` (`fixes.fillet_mitre_concave_face_corners`,
/// fork 4f06e72): each edge loses (convex) or gains (concave) the spandrel r²(1 - π/4) along its
/// length, and in the quadrant beyond both walls the mitred rounds take or add r³(5/3 - π/2): a
/// 60 x 40 x 10 plate's 30 x 20 x 5 pocket rim at r2 (15 faces) and r5 (11), a 60 x 60 x 5 plate's
/// 20 x 20 x 15 boss root at r4 (15). And the pocket rim chamfered 5.5, past the pocket's depth:
/// 21000 - (50 d² + 4/3 d³).
#[test]
fn rounds_at_a_concave_face_corner_are_mitred() {
    let made = |a: Body, b: Body, op| boolean(&a, &b, op).unwrap().unwrap();
    let pocket =
        || made(make_box([0.0; 3], [60.0, 40.0, 10.0]).unwrap(), make_box([15.0, 10.0, 5.0], [30.0, 20.0, 6.0]).unwrap(), BooleanOp::Subtract);
    let boss = || made(make_box([0.0; 3], [60.0, 60.0, 5.0]).unwrap(), make_box([20.0, 20.0, 5.0], [20.0, 20.0, 15.0]).unwrap(), BooleanOp::Union);
    let rim = [[30.0, 10.0, 10.0], [30.0, 30.0, 10.0], [15.0, 20.0, 10.0], [45.0, 20.0, 10.0]];
    let root = [[30.0, 20.0, 5.0], [40.0, 30.0, 5.0], [30.0, 40.0, 5.0], [20.0, 30.0, 5.0]];
    let want = |v0: f64, length: f64, r: f64, sign: f64| v0 + sign * (length * r * r * (1.0 - PI / 4.0) + 4.0 * r * r * r * (5.0 / 3.0 - PI / 2.0));
    for (body, edges, r, v, faces) in [
        (pocket(), rim, 2.0, want(21000.0, 100.0, 2.0, -1.0), 15),
        (pocket(), rim, 5.0, want(21000.0, 100.0, 5.0, -1.0), 11),
        (boss(), root, 4.0, want(24000.0, 80.0, 4.0, 1.0), 15),
    ] {
        let b = blend(&body, Blend::Round, &edges, &[r], 0.1, &[], &[], Corners::Filled).unwrap();
        native(&to_arrays(&b).unwrap()).unwrap();
        let got = volume(&b, 1e-12).unwrap();
        println!("mitred r{r}: {} faces, {got:.9}, expected {v:.9}", counts(&b).unwrap()[0]);
        assert_eq!(counts(&b).unwrap()[0], faces, "r{r}");
        assert!(rel(got, v) < 2e-8, "r{r}: {got} vs {v}");
    }
    // Through the flat arrays, as bodies reach a blend in the app (pocket_chamfer_on_a_boolean_body).
    let d = 5.5;
    let flat = super::from_arrays(&to_arrays(&pocket()).unwrap()).unwrap();
    let b = blend(&flat, Blend::Chamfer, &rim, &[d], 0.1, &[], &[], Corners::Filled).unwrap();
    let (got, v) = (volume(&b, 1e-8).unwrap(), 21000.0 - (50.0 * d * d + 4.0 / 3.0 * d * d * d));
    println!("mitred chamfer {d}: {got:.9}, expected {v:.9}");
    assert!(rel(got, v) < 1e-8, "{got} vs {v}");
}

/// Solidcore alone: edge attributes across a cliff overflow. A 40 x 30 x 10 block notched below its
/// top at (40, 0), its front and right top edges rounded r8 (their rails run off over the notch).
/// The back and left top edges are each trimmed 8 by those rounds: (0, 30)-(32, 30) and (0, 8)-(0, 30)
/// at z = 10, and each piece keeps its edge's attribute (`Behavior::Copy`), as on the fork.
#[test]
#[ignore = "solidcore bug: SubtractRemovalRegion loses the attributes of target edges it replaces (next-pin/bugs/cliff-overflow-loses-edge-attributes.md)"]
fn an_edge_tag_crosses_a_cliff_overflow() {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::geom::attribute::{Attribute, Behavior};
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::topology::{Brep, EdgeId, edge, vertex};
    use solidcore::{EntityRef, Kernel, Point3};
    fn between(b: &Brep, a: Point3, q: Point3) -> Option<EdgeId> {
        b.edges().ids().find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let (s, t) = (vertex::point(b, s.unwrap()).unwrap(), vertex::point(b, t.unwrap()).unwrap());
            (s.distance(a) < 1e-6 && t.distance(q) < 1e-6) || (s.distance(q) < 1e-6 && t.distance(a) < 1e-6)
        })
    }
    let p = Point3::new;
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    let block = create_box(&mut k, Point3::ZERO, 40.0, 30.0, 10.0).unwrap();
    let notch = create_box(&mut k, p(35.0, -1.0, -1.0), 6.0, 6.0, 4.0).unwrap();
    let mut b = boolean_difference(&mut k, block, notch).unwrap();
    const ID: u64 = 0x7000_0001;
    let edge_at = |b: &Brep, a, q| between(b, a, q).unwrap();
    for (a, q, v) in [(p(0.0, 30.0, 10.0), p(40.0, 30.0, 10.0), 1), (p(0.0, 0.0, 10.0), p(0.0, 30.0, 10.0), 2)] {
        let e = edge_at(&b, a, q);
        b.add_attribute(EntityRef::Edge(e), Attribute::long(ID, v, Behavior::Copy)).unwrap();
    }
    let rounds = [edge_at(&b, p(0.0, 0.0, 10.0), p(40.0, 0.0, 10.0)), edge_at(&b, p(40.0, 0.0, 10.0), p(40.0, 30.0, 10.0))];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &rounds, 8.0, 1, 1, 1.0).unwrap();
    s.do_fillet(&mut k).unwrap();
    drop(s);
    b.refresh_cached_topology();
    for (a, q, which) in [(p(0.0, 30.0, 10.0), p(32.0, 30.0, 10.0), "back"), (p(0.0, 8.0, 10.0), p(0.0, 30.0, 10.0), "left")] {
        let e = edge_at(&b, a, q);
        assert!(b.entity_attributes(EntityRef::Edge(e)).iter().any(|(_, x)| x.attribute_id() == ID), "the {which} edge lost its attribute");
    }
}

/// Solidcore alone: the pocket rim chamfered 5.5, as `fillet_mitre_concave_face_corners`' own test
/// does it, with `keep_operand_uv_trims` on too, on the body the boolean gives:
/// 21000 - (50 d² + 4/3 d³) = 19265.666666667 (the fork, and the same body through the flat
/// arrays). Refused: error 3, Corner Edge Computation Failure.
#[test]
#[ignore = "solidcore bug: next-pin/bugs/chamfer-boolean-body-keep-operand-uv-trims.md"]
fn pocket_chamfer_on_a_boolean_body() {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::topology::{EdgeId, edge, vertex};
    use solidcore::{Kernel, Point3};
    let p = Point3::new;
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_mitre_concave_face_corners = true;
    k.fixes.keep_operand_uv_trims = true;
    let plate = create_box(&mut k, Point3::ZERO, 60.0, 40.0, 10.0).unwrap();
    let cut = create_box(&mut k, p(15.0, 10.0, 5.0), 30.0, 20.0, 6.0).unwrap();
    let mut b = boolean_difference(&mut k, plate, cut).unwrap();
    let c = [p(15.0, 10.0, 10.0), p(45.0, 10.0, 10.0), p(45.0, 30.0, 10.0), p(15.0, 30.0, 10.0)];
    let on = |x: Point3, a: Point3, q: Point3| x.distance(a) < 1e-9 || x.distance(q) < 1e-9;
    let ids: Vec<EdgeId> = (0..4)
        .map(|i| {
            let (a, q) = (c[i], c[(i + 1) % 4]);
            b.edges()
                .ids()
                .find(|&e| {
                    let (s, t) = edge::start_end_vertices(&b, e).unwrap();
                    on(vertex::point(&b, s.unwrap()).unwrap(), a, q) && on(vertex::point(&b, t.unwrap()).unwrap(), a, q) && s != t
                })
                .unwrap()
        })
        .collect();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &ids, 5.5, 0, 1, 1.0).unwrap();
    let r = s.do_fillet(&mut k);
    assert!(r.is_ok(), "{} {}", s.error_info().code, s.error_info().comment);
    drop(s);
    b.refresh_cached_topology();
    let v = volume(&Body::new(b), 1e-8).unwrap();
    let d: f64 = 5.5;
    assert!(rel(v, 21000.0 - (50.0 * d * d + 4.0 / 3.0 * d * d * d)) < 1e-8, "{v}");
}

/// The corner of r5 rounds at (60, 40, 20) on a 60 x 40 x 20 box, set back 8 and left open: the
/// box's six faces and the three rounds (9 faces, 30 edges, 22 vertices), a sheet, and one hole
/// of twelve edges (each section and face curve in two), chained end to start, each beside a face
/// of the sheet. With SMLib built, the C++ kernel gives the same counts and the same hole.
#[test]
fn a_setback_corner_left_open_returns_its_hole() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    let edges = [[30.0, 40.0, 20.0], [60.0, 20.0, 20.0], [60.0, 40.0, 10.0]];
    let (open, holes) = timed("setback_open", move || {
        let b = Shape::Box([0.0; 3], [60.0, 40.0, 20.0]).core();
        blend_holes(&b, Blend::Round, &edges, &[5.0; 3], 0.1, &[[60.0, 40.0, 20.0]], &[8.0], Corners::Open).unwrap()
    });
    assert_eq!(counts(&open).unwrap(), [9, 30, 22]);
    assert!(!super::is_manifold_solid(&open).unwrap());
    let [hole] = holes.as_slice() else { panic!("one corner: {holes:?}") };
    assert_eq!(hole.edges.len(), 12);
    let flat = to_arrays(&open).unwrap();
    let (rec, _) = native(&flat).unwrap();
    rec.validate().unwrap();
    let ends = &flat.ints[&crate::flat::EDGE_VERTICES];
    let end = |h: &crate::HoleEdge, last: bool| ends[2 * h.edge + usize::from(h.forward == last)];
    for (k, h) in hole.edges.iter().enumerate() {
        let next = &hole.edges[(k + 1) % 12];
        assert_eq!(end(h, true), end(next, false), "edge {k} ends where the next starts");
        assert!(h.face < 9);
    }
    #[cfg(feature = "native")]
    {
        crate::init().unwrap();
        let b = crate::ffi::make_box(&[0.0; 3], &[60.0, 40.0, 20.0]).map(crate::Brep::made).unwrap();
        let (h, values) = crate::ffi::ops::blend_setback_open(
            crate::ffi::ops::BLEND_ROUND,
            b.cpp().unwrap(),
            &edges,
            &[5.0; 3],
            &[[60.0, 40.0, 20.0]],
            &[8.0],
            0.1,
        )
        .unwrap();
        assert_eq!(crate::ffi::counts(&h).unwrap(), [9, 30, 22], "C++ counts");
        let ours: Vec<i64> =
            std::iter::once(12).chain(hole.edges.iter().flat_map(|h| [h.edge as i64, h.face as i64, i64::from(h.forward)])).collect();
        assert_eq!(values, ours, "the hole (C++ vs solidcore)");
    }
    // A corner where an edge is not rounded builds no setback, and the refusal names it.
    let refused = timed("setback_open_refused", move || {
        let b = Shape::Box([0.0; 3], [60.0, 40.0, 20.0]).core();
        blend_holes(&b, Blend::Round, &edges[..2], &[5.0; 2], 0.1, &[[60.0, 40.0, 20.0]], &[8.0], Corners::Open).err()
    });
    assert_eq!(refused, Some(Error::CornerRefused { corner: 0 }));
}

/// The capi's argument checks, before any kernel call.
#[test]
fn hostile_arguments_are_refused() {
    let b = BLOCK.core();
    let none = Corners::Filled;
    assert_eq!(blend(&b, Blend::Round, &[], &[1.0], TOL, &[], &[], none).err(), Some(Error::Argument));
    assert_eq!(blend(&b, Blend::Round, &[[f64::NAN; 3]], &[1.0], TOL, &[], &[], none).err(), Some(Error::Argument));
    assert_eq!(blend(&b, Blend::Round, &[TOP_FRONT], &[-1.0], TOL, &[], &[], none).err(), Some(Error::Argument));
    assert_eq!(blend(&b, Blend::Round, &[TOP_FRONT], &[1.0], 0.0, &[], &[], none).err(), Some(Error::Argument));
    assert_eq!(blend(&b, Blend::Round, &[TOP_FRONT], &[f64::INFINITY], TOL, &[], &[], none).err(), Some(Error::Argument));
    // A chamfer's corners are bevelled, which builds no setback.
    assert_eq!(blend(&b, Blend::Chamfer, &[TOP_FRONT], &[1.0], TOL, &[[0.0, 0.0, 10.0]], &[1.0], none).err(), Some(Error::Unsupported));
    assert_eq!(fillet_variable(&b, TOP_FRONT, TOL, [0.0; 3], 1, &[0.0, 1.0], &[1.0, 2.0]).err(), Some(Error::Argument));
    // A radius that pinches to nothing inside the edge.
    assert_eq!(fillet_variable(&b, TOP_FRONT, TOL, [0.0; 3], 1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 0.0, 1.0]).err(), Some(Error::Unsupported));
    assert_eq!(chamfer_sides(&b, &[TOP_FRONT], TOL, 1.0, ChamferSecond::Angle(4.0), [0.0, 0.0, 1.0]).err(), Some(Error::Argument));
    assert_eq!(chamfer_sides(&b, &[TOP_FRONT], TOL, 1.0, ChamferSecond::Distance(1.0), [0.0; 3]).err(), Some(Error::Argument));
}

/// A setback longer than an edge at its corner leaves no room, before any blend is made.
#[test]
fn a_setback_past_an_edge_has_no_room() {
    let r = timed("setback_long", || {
        let b = BLOCK.core();
        let corner = [[15.0, 20.0, 10.0], [30.0, 10.0, 10.0], [30.0, 20.0, 5.0]];
        blend(&b, Blend::Round, &corner, &[2.0; 3], TOL, &[[30.0, 20.0, 10.0]], &[10.0], Corners::Filled).err()
    });
    assert_eq!(r, Some(Error::NoRoom));
}

/// The body blended is never changed.
#[test]
fn the_body_stays_as_it_was() {
    let r = timed("operand", || {
        let b = BLOCK.core();
        let _ = blend(&b, Blend::Round, &[TOP_FRONT], &[2.0], TOL, &[], &[], Corners::Filled);
        (counts(&b).unwrap(), volume(&b, 1e-8).unwrap())
    });
    assert_eq!(r.0, [6, 12, 8]);
    assert!(rel(r.1, ABC) < 1e-12, "{}", r.1);
}

/// The chamfer's first distance goes on the face leaning most along `first`: 2 along the top and
/// 3 down the front for +z, the other way round for -y. The volumes are the same; the centroid
/// tells them apart. The removed prism's section is the triangle with legs t (back along the
/// top, in y) and f (down the front, in z) at the edge (y = 0, z = 10), centroid
/// (t/3, 10 - f/3), area t f / 2: the block keeps ȳ = (6000·10 - 15 t f · t/3) / (6000 - 15 t f)
/// and z̄ = (6000·5 - 15 t f (10 - f/3)) / (6000 - 15 t f).
#[test]
fn the_first_distance_goes_on_the_face_along_first() {
    for (first, top, front) in [([0.0, 0.0, 1.0], 2.0, 3.0), ([0.0, -1.0, 0.0], 3.0, 2.0)] {
        let r = timed("sides_first", move || {
            let b = BLOCK.core();
            chamfer_sides(&b, &[TOP_FRONT], TOL, 2.0, ChamferSecond::Distance(3.0), first).and_then(|c| super::mass_properties(&c, 1e-8))
        });
        let m = r.unwrap_or_else(|e| panic!("{first:?}: {e}"));
        let cut = 15.0 * top * front;
        let y = (6000.0 * 10.0 - cut * top / 3.0) / (6000.0 - cut);
        let z = (6000.0 * 5.0 - cut * (10.0 - front / 3.0)) / (6000.0 - cut);
        assert!(rel(m.volume, 6000.0 - cut) < 1e-9, "{first:?}: volume {}", m.volume);
        assert!((m.centroid[1] - y).abs() < 1e-7 && (m.centroid[2] - z).abs() < 1e-7, "{first:?}: centroid {:?} vs ({y}, {z})", m.centroid);
    }
}

/// Face and edge tags (`Brep::tagged`) through blends routed to solidcore, as on C++: the box is
/// tagged on the C++ kernel, its solidcore body converted from that and tagged the same way, and
/// the two results' histories must be the same.
#[cfg(feature = "native")]
mod history {
    use solidcraft_geom::brep::history::Change;

    use crate::history::Raw;
    use crate::{Blend, Brep, ffi};

    /// A 20 x 30 x 10 box, tagged as operand 0, with `kind` of `size` on its top front edge
    /// (y = 0, z = 10), on both kernels: solidcore's history, then the C++ kernel's.
    fn one_edge(kind: Blend, size: f64) -> (Raw, Raw) {
        crate::init().unwrap();
        let a = Brep::made(ffi::make_box(&[0.0; 3], &[20.0, 30.0, 10.0]).unwrap()).tagged(0).unwrap();
        let edge = [[10.0, 0.0, 10.0]];
        let code = match kind {
            Blend::Round => ffi::ops::BLEND_ROUND,
            Blend::Curvature => ffi::ops::BLEND_CURVATURE,
            Blend::Chord => ffi::ops::BLEND_CHORD,
            Blend::Chamfer => ffi::ops::BLEND_CHAMFER,
        };
        let cpp = ffi::ops::blend(code, a.cpp().unwrap(), &edge, &[size], super::TOL).unwrap();
        let cpp = Brep::made(cpp).history().unwrap();
        let core = super::blend(a.core().unwrap(), kind, &edge, &[size], super::TOL, &[], &[], super::Corners::Filled).unwrap();
        (Brep::made_core(core).history().unwrap(), cpp)
    }

    /// The changes of `h`'s faces, sorted: one blend of one box edge leaves the bottom and back
    /// as they were, trims the top, front and both sides, and makes the blend face.
    fn changes(h: &Raw) -> Vec<Change> {
        let mut c: Vec<Change> = h.faces.iter().map(|f| f.change).collect();
        c.sort_by_key(|c| *c as u8);
        c
    }

    fn check(kind: Blend, size: f64) {
        let (core, cpp) = one_edge(kind, size);
        let want = [Change::Kept, Change::Kept, Change::Modified, Change::Modified, Change::Modified, Change::Modified, Change::Generated];
        assert_eq!(changes(&cpp), want, "C++ {kind:?}: {cpp:?}");
        assert_eq!(core.canonical(), cpp.canonical(), "{kind:?}");
    }

    /// A 2 mm round on the top front edge of a box: the top and front are trimmed, the round is
    /// new.
    #[test]
    fn tags_cross_a_fillet() {
        let _cpp = crate::route::on_cpp();
        check(Blend::Round, 2.0);
    }

    /// A 2 mm chamfer on the same edge: the same faces kept, trimmed and made as the round.
    #[test]
    fn tags_cross_a_chamfer() {
        let _cpp = crate::route::on_cpp();
        check(Blend::Chamfer, 2.0);
    }
}

/// The capi's `knife_edge_chamfers` (fork e96898d): a 10 high extrusion of the profile whose arc
/// of radius R = 21 runs out tangent into its top line at the tip x = 62 1/6 (a knife edge, a
/// cusp), chamfered d at the tip. No ball lies d back along both faces there; the chamfer is the
/// chord between the contacts of the ball of radius d, sqrt(4 R d) back on the flat face and where
/// the line from the arc's axis to the ball's centre meets the wall. It takes the triangle flat
/// contact, tip, wall contact less the wall's circular segment R²/2 (b - sin b),
/// b = acos((R - d)/(R + d)): at d = 2 the chord is 3.822 wide and takes 159.23645 (each to
/// 1e-12). Mirrored in x too.
#[test]
fn knife_edge_chamfers() {
    use super::sweep::extrude;
    use super::transform::transform;
    use crate::sweeps::{Curve, DraftCorner};

    const R: f64 = 21.0;
    const TIP: f64 = 62.0 + 1.0 / 6.0;
    const H: f64 = 10.0;
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line { start: [a[0], a[1], 0.0], end: [b[0], b[1], 0.0] };
    let profile = [
        line([0.0, 0.0], [0.0, -R]),
        line([0.0, -R], [TIP - R, -R]),
        Curve::Arc { center: [TIP, -R, 0.0], axis: [0.0, 0.0, -1.0], reference: [1.0, 0.0, 0.0], radius: R, start: PI, end: 1.5 * PI },
        line([TIP, 0.0], [0.0, 0.0]),
    ];
    let part = extrude(&profile, [0.0, 0.0, 1.0], H, 0.0, DraftCorner::Curvature, true).unwrap();
    let before = (TIP - R) * R * H + R * R * (1.0 - PI / 4.0) * H;
    assert!((volume(&part, 1e-8).unwrap() - before).abs() < 1e-9 * before);
    for d in [1.0, 2.0, 5.0] {
        let flat = TIP - (4.0 * R * d).sqrt();
        let (wx, wy) = (TIP - (4.0 * R * d).sqrt() * R / (R + d), -R + (R - d) * R / (R + d));
        let triangle = 0.5 * ((TIP - flat) * wy).abs();
        let b = ((R - d) / (R + d)).acos();
        let removed = (triangle - 0.5 * R * R * (b - b.sin())) * H;
        if d == 2.0 {
            assert!((removed - 159.236_450_742_901_18).abs() < 1e-12 * 159.24);
            assert!(((wx - flat).hypot(wy) - 3.822_132_343_623_634_7).abs() < 1e-12 * 3.83);
        }
        for mirrored in [false, true] {
            let sx = if mirrored { -1.0 } else { 1.0 };
            let m = [[sx, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
            let body = transform(&part, &m).unwrap();
            let tip = [sx * TIP, 0.0, H / 2.0];
            let cut = blend(&body, Blend::Chamfer, &[tip], &[d], TOL, &[], &[], Corners::Filled).unwrap();
            let v = volume(&cut, 1e-8).unwrap();
            let want = before - removed;
            assert!((v - want).abs() < 1e-9 * want, "d {d} mirrored {mirrored}: {v} vs {want}");
            native(&to_arrays(&cut).unwrap()).unwrap();
        }
    }
}
