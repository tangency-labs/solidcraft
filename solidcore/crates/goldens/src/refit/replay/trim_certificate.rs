//! The certificate on the 9 rows the WR-493 retirement moves through a rebuilt UV trim
//! (`topology.edgeuse_p2` `surf_sphere150_*` ×8, `blend.fillet_standard_solver_p1`
//! `gen0183`).
//!
//! In each row the trim the edge use reports is a line of constant `u` of the sphere's NURB
//! (the B-rep's import or the op rebuilt it on that NURB), or the case's own input made from
//! it. Its `u` is one ulp apart between this branch (the file's NURB) and main (the fitted
//! NURB). The replay ([`super`]) proves the move comes only from that NURB; this proves both
//! trims are right on their own NURB. For each row and each side (as is, and with the refit
//! seam that reproduces main's build), the test bounds the distance between the face's
//! surface along the edge use's trim and the edge's 3D curve over its interval by
//! [`span_bound`]: the convex hull of the Bernstein coefficients of their difference, every
//! rounding included (`nurbs::certificate` module docs). No point is sampled. The bound holds
//! for any correspondence of the two parameters, so it bounds both directed Hausdorff
//! distances; the test asserts it is within the edge's tolerance:
//! 1. for the trim the op starts from (the B-rep as the case makes it);
//! 2. for the trim the row reports, after the op, tied to the row: its encoding is a run of
//!    the row's values, bit for bit. One row (`rb_cs2_2_0`, a check without adjustment)
//!    reports the check's input instead, the trim of 1. moved by the case's `(du, dv)` of
//!    the face's domain (`topology_edgeuse_p2::uv_curve`): off the surface by construction,
//!    so no tolerance applies. For it the test asserts the reported trim is exactly that
//!    move of the certified trim, so its moved values are the certified trim's move carried
//!    through the case's offset.
//!
//! The trims are of degree 1: their vertices are their control points, read, not evaluated,
//! and the surface along each segment is the line between them in (u, v), the form
//! [`span_bound`] takes.

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::nurbs::certificate::{span_bound, Node, RailSpans};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, face, Brep, EdgeuseId};

use super::super::state::{self, Refit};
use super::{cases, unit};
use crate::output::Output;
use crate::util::trim_curve::put_trim_curve;
use crate::Case;

/// The rows: unit and case.
const ROWS: [(&str, &str); 9] = [
    ("topology.edgeuse_p2", "surf_sphere150_cs0_4_1"),
    ("topology.edgeuse_p2", "surf_sphere150_rb0"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_0_0"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_0_1"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_2_0"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_cs2_2_1"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_nt2_10"),
    ("topology.edgeuse_p2", "surf_sphere150_rb_nt2_15"),
    ("blend.fillet_standard_solver_p1", "gen0183"),
];

/// The row that reports the check's input, the edge use's trim moved by the case's offset.
const MOVED_INPUT: [&str; 1] = ["surf_sphere150_rb_cs2_2_0"];

/// The case's B-rep and edge use, before the op or after it.
fn brep(unit_id: &str, case: &Case, after: bool) -> (Brep, EdgeuseId) {
    let r = match (unit_id, after) {
        ("topology.edgeuse_p2", false) => crate::units::topology_edgeuse_p2::scene(case),
        ("topology.edgeuse_p2", true) => crate::units::topology_edgeuse_p2::after_op(case),
        (_, after) => crate::units::blend_fillet_standard_solver_p1::eu_case(case, after),
    };
    r.unwrap_or_else(|e| panic!("{}: {e:?}", case.name))
        .unwrap_or_else(|| panic!("{}: no B-rep", case.name))
}

/// The edge use's trim.
fn trim(b: &Brep, eu: EdgeuseId) -> &Curve {
    eu1::uv_trim_curve(b, eu).unwrap().0.unwrap()
}

/// The vertices of a degree-1 trim with their parameters (a vertex of a clamped degree-1
/// B-spline is its control point, at the knot after it).
fn polyline(c: &Curve) -> Vec<(f64, Point2)> {
    let base = match c.kind() {
        CurveKind::BSpline(b) => std::borrow::Cow::Borrowed(b),
        CurveKind::Line(l) => l.base_with_nurb().unwrap(),
        _ => panic!("a trim neither a B-spline nor a line"),
    };
    let nurb = base.gw_nurb().unwrap();
    assert_eq!(nurb.degree(), 1, "a trim not of degree 1");
    assert!(!nurb.is_rational());
    let cpts = nurb.cpts().get(..=nurb.n()).unwrap();
    cpts.iter()
        .zip(nurb.knots().iter().skip(1))
        .map(|(q, &s)| (s, Point2::new(q.x, q.y)))
        .collect()
}

/// The certified bound of the distance between the face's surface along the edge use's trim
/// and the edge's 3D curve over its interval, and the edge's tolerance. The correspondence
/// is linear between the trim's vertices and the edge's interval, in either orientation
/// ([`span_bound`] halves a piece above the tolerance at the rail's middle); each orientation
/// is a correspondence, so each bound it gives holds, and the least is taken (one that
/// cannot be bounded gives none).
fn trim_bound(k: &Kernel, b: &Brep, eu: EdgeuseId) -> (f64, f64) {
    let f = eu1::face(b, eu).unwrap().unwrap();
    let surf = face::p2::surface(b, f).unwrap().unwrap();
    let e = eu1::edge(b, eu).unwrap().unwrap();
    let tol = edge::tolerance(b, e).unwrap();
    let rail = edge::curve(b, e).unwrap().unwrap();
    let CurveKind::BSpline(rb) = rail.kind() else {
        panic!("an edge curve not a B-spline");
    };
    let i = edge::interval(b, e).unwrap();
    let p = rail.evaluate_point(k, i.min()).unwrap();
    let spans = RailSpans::about(rb.gw_nurb().unwrap(), p).unwrap();
    let vs = polyline(trim(b, eu));
    let (s0, s1) = (vs.first().unwrap().0, vs.last().unwrap().0);
    let mut best = f64::INFINITY;
    for reversed in [false, true] {
        let t_at = |s: f64| {
            let f = (s - s0) / (s1 - s0);
            let f = if reversed { 1.0 - f } else { f };
            i.min() + f * (i.max() - i.min())
        };
        let mut worst = 0.0_f64;
        for w in vs.windows(2) {
            let a = Node {
                t: t_at(w[0].0),
                uv: w[0].1,
            };
            let z = Node {
                t: t_at(w[1].0),
                uv: w[1].1,
            };
            let d = span_bound(k, surf, rail, &spans, p, a, z, tol).unwrap_or(f64::INFINITY);
            worst = worst.max(d);
        }
        best = best.min(worst);
    }
    (best, tol)
}

/// The encoding of the edge use's trim, asserted to be a run of the row's values, bit for bit.
fn tied(k: &Kernel, b: &Brep, eu: EdgeuseId, out: &Output) -> Vec<u64> {
    let mut enc = Vec::new();
    put_trim_curve(k, &mut enc, Some(trim(b, eu))).unwrap();
    let bits: Vec<u64> = enc.iter().map(|x| x.to_bits()).collect();
    let found = out.values.values().any(|v| {
        let v: Vec<u64> = v.iter().map(|x| x.to_bits()).collect();
        v.windows(bits.len()).any(|w| w == bits.as_slice())
    });
    assert!(found, "the trim {enc:?} is not in the row");
    bits
}

/// The trim after a check of the case's moved input is the trim before it moved by the
/// case's `(du, dv)` of the face's domain, as `topology_edgeuse_p2::uv_curve` makes it.
fn assert_moved_input(case: &Case, before: (&Brep, EdgeuseId), after: (&Brep, EdgeuseId)) {
    let uv = &case.inputs["uv"];
    assert_eq!(case.inputs["op"], "check_set");
    assert_eq!(uv["src"], "current");
    let (du, dv) = (uv["du"].as_f64().unwrap(), uv["dv"].as_f64().unwrap());
    assert!(du != 0.0 || dv != 0.0);
    let f = eu1::face(before.0, before.1).unwrap().unwrap();
    let d = before.0.face(f).unwrap().uv_domain;
    let (sx, sy) = (du * (d.max().x - d.min().x), dv * (d.max().y - d.min().y));
    let moved: Vec<(f64, f64)> = polyline(trim(before.0, before.1))
        .iter()
        .map(|(_, q)| (q.x + sx, q.y + sy))
        .collect();
    let reported: Vec<(f64, f64)> = polyline(trim(after.0, after.1))
        .iter()
        .map(|(_, q)| (q.x, q.y))
        .collect();
    assert_eq!(
        moved, reported,
        "{}: not the trim moved by the case's offset",
        case.name
    );
}

/// Both trims of each row, this branch's on the file's NURB and main's on the fitted NURB,
/// are within the edge's tolerance of its curve, by a certified bound; the trim the row
/// reports is one of them, or one of them moved by the case's own offset.
#[test]
fn wr493_rebuilt_trims_are_within_the_edge_tolerance_on_both_nurbs() {
    let k = crate::runner_kernel();
    let mut report = Vec::new();
    for (unit_id, name) in ROWS {
        let u = unit(unit_id);
        let all = cases(u.as_ref());
        let case = &all[name];
        let mut reported = Vec::new();
        for refit in [false, true] {
            if refit {
                state::set(Some(Refit::default()));
            }
            let out = u.run_rust(case).unwrap();
            let before = brep(unit_id, case, false);
            let after = brep(unit_id, case, true);
            if refit {
                let r = state::take().expect("refit lost");
                assert!(r.replaced > 0 && r.failures.is_empty(), "{name}: {r:?}");
            }
            let side = ["new", "main"][usize::from(refit)];
            reported.push(tied(&k, &after.0, after.1, &out));
            let (d, tol) = trim_bound(&k, &before.0, before.1);
            report.push(format!(
                "{name} {side} before the op: {d:e} (tolerance {tol:e})"
            ));
            assert!(d <= tol, "{name} {side}: bound {d:e} above {tol:e}");
            if MOVED_INPUT.contains(&name) {
                assert_moved_input(case, (&before.0, before.1), (&after.0, after.1));
                continue;
            }
            let (d, tol) = trim_bound(&k, &after.0, after.1);
            report.push(format!("{name} {side} reported: {d:e} (tolerance {tol:e})"));
            assert!(
                d <= tol,
                "{name} {side} reported: bound {d:e} above {tol:e}"
            );
        }
        // the two reported trims differ: the certificate covers the trim the row moves
        assert_ne!(reported[0], reported[1], "{name}: the trims are the same");
    }
    eprintln!("{}", report.join("\n"));
}
