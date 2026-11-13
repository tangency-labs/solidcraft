//! A setback measured along an arc (#3184): the edge's length is a certified bracket, the
//! size named (`SetbackTooLong { largest }`, the overlap check) is taken from its lower side,
//! and a setback is refused as soon as it is above that. A ring sector 30 degrees round, outer
//! radius 3, inner 0.5, 3 high: at its top outer corner the outer arc, 3 pi / 6 = 1.571 long,
//! is the shortest edge. The named size passes the overlap check and 1e-9 above it is
//! refused by it, though the arc's bracket is wider than 1e-9.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::blend_refusal::{BlendRefusal, SetbackTooLongWhy};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::{edge, Brep, EdgeId, VertexId};

const OUTER: f64 = 3.0;
const INNER: f64 = 0.5;
const SWEEP: f64 = 30.0;
const HEIGHT: f64 = 3.0;

fn kernel() -> Kernel {
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

fn pick(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, p) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn at(r: f64, deg: f64, z: f64) -> Point3 {
    let a = deg.to_radians();
    Point3::new(r * a.cos(), r * a.sin(), z)
}

fn sector(k: &mut Kernel) -> Brep {
    let line = |a: Point3, b: Point3| Curve::from(create_line_segment(a, b).unwrap());
    let mut inner = create_arc(Point3::new(0.0, 0.0, 0.0), INNER, 0.0, SWEEP).unwrap();
    let ivl = inner.natural_interval();
    inner.reverse_parameterization(&ivl).unwrap();
    let profile = [
        line(at(INNER, 0.0, 0.0), at(OUTER, 0.0, 0.0)),
        Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), OUTER, 0.0, SWEEP).unwrap()),
        line(at(OUTER, SWEEP, 0.0), at(INNER, SWEEP, 0.0)),
        Curve::from(inner),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), HEIGHT, true).unwrap()
}

/// The outer top corner rounded 0.2 on its three edges and set back `setback` there.
fn run(setback: f64) -> (Result<(), SolidError>, VertexId) {
    let mut k = kernel();
    let mut b = sector(&mut k);
    let corner = at(OUTER, 0.0, HEIGHT);
    let picks = [
        at(OUTER, 0.5 * SWEEP, HEIGHT),
        at(0.5 * (OUTER + INNER), 0.0, HEIGHT),
        at(OUTER, 0.0, 0.5 * HEIGHT),
    ];
    let edges: Vec<EdgeId> = picks.iter().map(|&p| pick(&k, &b, p)).collect();
    let v = b
        .vertices()
        .ids()
        .find(|&v| b.vertex(v).unwrap().point.distance(corner) < 1e-9)
        .unwrap();
    let xs = vec![FilletSurfaceGeneratorType::Circular; 3];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius(&mut k, &edges, &[0.2; 3], &xs, 1, 1.0)
        .unwrap();
    s.add_set_back_vertex(v);
    s.set_set_back_fill(v, setback, 0.0, false);
    s.create_corners(&k, None).unwrap();
    for c in s.fillet_corners().unwrap() {
        if s.corner(c).unwrap().vertex == Some(v) {
            s.set_corner_setback(c, setback).unwrap();
        }
    }
    (s.do_filleting(&mut k, None), v)
}

fn overlap_named(r: &Result<(), SolidError>, v: VertexId) -> Option<f64> {
    match r {
        Err(SolidError::BlendRefused {
            refusal:
                BlendRefusal::SetbackTooLong {
                    vertex,
                    largest,
                    why: SetbackTooLongWhy::Overlap,
                },
            ..
        }) if *vertex == v => Some(*largest),
        _ => None,
    }
}

#[test]
fn a_setback_on_an_arc_passes_at_the_size_named_and_is_refused_just_above() {
    // the arc's bracket, asked as the overlap check asks it, is wider than 1e-9
    let k = kernel();
    let b = sector(&mut kernel());
    let arc = pick(&k, &b, at(OUTER, 0.5 * SWEEP, HEIGHT));
    let curve = edge::evaluation_curve(&b, arc).unwrap().unwrap();
    let ivl = edge::interval(&b, arc).unwrap();
    let exact = OUTER * SWEEP.to_radians();
    let bracket = curve.length_bound(&k, &ivl, 1e-8).unwrap();
    println!(
        "ARC bracket {:e} wide, {bracket:?}, exact {exact}",
        bracket.upper - bracket.lower
    );
    assert!(bracket.lower <= exact && exact <= bracket.upper);
    // asked the whole arc: refused, naming the size from its lower side
    let (r, v) = run(exact);
    let named = overlap_named(&r, v).expect("refused by the overlap check");
    assert!(named < exact, "{named}");
    // the named size passes the overlap check (the corner itself, a setback of nearly the
    // whole arc, then fails to finish here with or without #3184: measured with the check
    // swapped out); 1e-9 above it is refused by the overlap check
    let (r, v) = run(named);
    assert!(
        overlap_named(&r, v).is_none(),
        "the named size refused by the overlap check"
    );
    let (r, v) = run(named + 1e-9);
    assert!(
        overlap_named(&r, v).is_some(),
        "1e-9 above the named size passed"
    );
}
