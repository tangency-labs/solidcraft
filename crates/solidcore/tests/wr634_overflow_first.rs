//! WR-634 (the fork's tangency-labs/smlib@241f7ba): the executive's overflow-first hooks.
//! With `fillet_refuse_rails_leaving_faces` and `fillet_cliff_overflow`, a session set to
//! classify only (`set_classify_only`), or to put overflows first (`set_overflow_first`)
//! when blends of several edges overflow, stops after classifying the rails, builds
//! nothing, reports `FilletErrorType::OverflowFirst` and the edges whose blends overflow
//! (`overflow_edges`). The ordering is the caller's; the last test emulates the fork's C
//! API ordering with the hooks on its case: R21 on a 20 cube's edge with R5 on the
//! corner's other two edges.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn kernel(cliff: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = cliff;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = cliff;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The edge from `p` to `q` (either way), if there is one.
fn find_edge(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> Option<EdgeId> {
    b.edges().ids().find(|&e| {
        let ivl = edge::interval(b, e).unwrap();
        let c = edge::curve(b, e).unwrap().unwrap();
        let s = c.evaluate_point(k, ivl.min()).unwrap();
        let t = c.evaluate_point(k, ivl.max()).unwrap();
        (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
            || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
    })
}

const O: Point3 = Point3::ZERO;
const X: Point3 = Point3::new(20.0, 0.0, 0.0);
const Y: Point3 = Point3::new(0.0, 20.0, 0.0);
const Z: Point3 = Point3::new(0.0, 0.0, 20.0);

fn cube(k: &mut Kernel) -> Brep {
    create_box(k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap()
}

/// The cube's edges at the origin: along x (R21 in the fork's case), along y and z.
fn corner_edges(k: &Kernel, b: &Brep) -> [EdgeId; 3] {
    [
        find_edge(k, b, O, X).unwrap(),
        find_edge(k, b, O, Y).unwrap(),
        find_edge(k, b, O, Z).unwrap(),
    ]
}

/// What a session reports: the error code, the overflowing edges and whether
/// `do_fillet` succeeded.
struct Run {
    ok: bool,
    code: i32,
    overflow: Vec<EdgeId>,
}

/// Rounds `edges` of `b` by `radii` with the hooks set as given.
fn run(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    radii: &[f64],
    classify_only: bool,
    overflow_first: bool,
) -> Run {
    let mut s = FilletSession::new(k, b).unwrap();
    s.set_classify_only(classify_only);
    s.set_overflow_first(overflow_first);
    s.prepare_per_edge_constant_radius(
        k,
        edges,
        radii,
        &[FilletSurfaceGeneratorType::Circular],
        1,
        1.0,
    )
    .unwrap();
    let ok = s.do_fillet(k).is_ok();
    Run {
        ok,
        code: s.error_info().code,
        overflow: s.overflow_edges(),
    }
}

const STOP: i32 = FilletErrorType::OverflowFirst as i32;

/// The closed form of R21 alone on a 20 cube's edge: the cube less the section outside
/// the circle of radius 21 about (21, 21) in the 20 x 20 square, along 20.
fn r21_closed_form() -> f64 {
    let (r, w, h) = (21.0_f64, 20.0_f64, 20.0_f64);
    let a = w.min(r);
    let f = |s: f64| s / 2.0 * (r * r - s * s).max(0.0).sqrt() + r * r / 2.0 * (s / r).asin();
    let u_h = r - (r * r - (r - h) * (r - h)).sqrt();
    let area = h * u_h.min(a) + r * (a - u_h) - (f(r - u_h) - f(r - a));
    8000.0 - 20.0 * area
}

#[test]
fn classify_only_stops_after_classification_and_reports_overflowing_edges() {
    let mut k = kernel(true);
    let mut b = cube(&mut k);
    let [ex, ey, ez] = corner_edges(&k, &b);
    // each edge alone: only the R21 overflows its faces
    let r = run(&mut k, &mut b, &[ex], &[21.0], true, false);
    assert!(!r.ok);
    assert_eq!(r.code, STOP);
    assert_eq!(r.overflow, vec![ex]);
    for e in [ey, ez] {
        let r = run(&mut k, &mut b, &[e], &[5.0], true, false);
        assert!(!r.ok, "classify-only stops even when the blend fits");
        assert_eq!(r.code, STOP);
        assert!(r.overflow.is_empty());
    }
    // nothing was built: the cube is untouched
    assert_eq!(b.faces().ids().count(), 6);
    assert!((volume(&k, &mut b) - 8000.0).abs() < 1e-6);
}

#[test]
fn overflow_first_stops_when_blends_of_several_edges_overflow() {
    let mut k = kernel(true);
    let mut b = cube(&mut k);
    let edges = corner_edges(&k, &b);
    let r = run(&mut k, &mut b, &edges, &[21.0, 5.0, 5.0], false, true);
    assert!(!r.ok);
    assert_eq!(r.code, STOP);
    assert!(r.overflow.contains(&edges[0]), "{:?}", r.overflow);
    assert_eq!(b.faces().ids().count(), 6);
    // one edge alone is never stopped by overflow-first: it is built
    let r = run(&mut k, &mut b, &edges[..1], &[21.0], false, true);
    assert!(r.ok, "code {}", r.code);
    assert!(r.overflow.is_empty());
}

#[test]
fn hooks_off_or_without_the_flags_change_nothing() {
    // hooks off: the three together are not stopped
    let mut k = kernel(true);
    let mut b = cube(&mut k);
    let edges = corner_edges(&k, &b);
    let r = run(&mut k, &mut b, &edges, &[21.0, 5.0, 5.0], false, false);
    assert_ne!(r.code, STOP);
    assert!(r.overflow.is_empty());
    // without the cliff overflow flags the hooks do nothing: a fitting blend builds
    let mut k = kernel(false);
    let mut b = cube(&mut k);
    let edges = corner_edges(&k, &b);
    let r = run(&mut k, &mut b, &edges[1..2], &[5.0], true, true);
    assert!(r.ok, "code {}", r.code);
    assert!(r.overflow.is_empty());
}

/// The fork's end-to-end case (the C API test `ops_kernel` `overflow_goes_first`), its ordering
/// emulated with the hooks: classify each edge alone, blend those that overflow on their
/// own first, then what is left of the others. The R21 takes the whole of both faces the
/// R5 edges lie on, so they are gone and the result is the R21's alone.
#[test]
fn overflow_goes_first_on_a_cube_corner_matches_the_closed_form() {
    let mut k = kernel(true);
    let mut b = cube(&mut k);
    let edges = corner_edges(&k, &b);
    let radii = [21.0, 5.0, 5.0];
    // together, they stop: some overflow
    let r = run(&mut k, &mut b, &edges, &radii, false, true);
    assert_eq!(r.code, STOP);
    // each alone
    let mut first = Vec::new();
    let mut rest = Vec::new();
    for (&e, &radius) in edges.iter().zip(&radii) {
        let r = run(&mut k, &mut b, &[e], &[radius], true, false);
        assert_eq!(r.code, STOP);
        if r.overflow.is_empty() {
            rest.push(radius);
        } else {
            first.push((e, radius));
        }
    }
    assert_eq!(first, vec![(edges[0], 21.0)]);
    assert_eq!(rest, vec![5.0, 5.0]);
    // the overflow first
    let (e, radius) = first[0];
    let r = run(&mut k, &mut b, &[e], &[radius], false, true);
    assert!(r.ok, "code {}", r.code);
    assert!(is_manifold_solid(&b).unwrap());
    // the R5 edges are gone with the faces they lay on
    assert!(find_edge(&k, &b, O, Y).is_none());
    assert!(find_edge(&k, &b, O, Z).is_none());
    let v = volume(&k, &mut b);
    let expected = r21_closed_form();
    assert!(
        (v - expected).abs() <= 1e-9 * expected,
        "volume {v} against {expected}"
    );
}
