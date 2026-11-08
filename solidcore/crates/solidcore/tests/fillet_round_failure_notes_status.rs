//! A round that fails notes why (#11360's `a_round_ending_in_an_earlier_round_matches_both_together`,
//! "fillet status 1001 on the round that ends in the earlier one"). The part: a 41 1/6 x 21
//! rectangle and, beside it, what a circle of radius 21 about (62 1/6, -21) leaves against the
//! top line it touches, extruded 10 (an arc wall square to the front, a knife edge at its tip).
//! One edge's tangent chain is rounded 2, then another's on the result, for that test's ten
//! (first, then) pairs, and the part's own edges of both together. Where a round fails, the
//! session's error info names it: a nonzero code and a comment.
//!
//! With `offset_extension_from_base`, `fillet_refuse_overlapping_bands` and
//! `offset_extension_as_itself` (the least set under which it shows; any one of them off and the
//! round builds), the arc's round followed by the front edge's fails in the rollover
//! (`SM_ERR_NULL_POINTER` from `blend::fillet_solver::p5`) and noted nothing: code 0 and no
//! comment. It now notes `SM_FILERR_EDGE_PROBLEM` (4) with what failed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::curve::curve::Curve;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::api_queries::{get_edges, surface_closest_point};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, vertex, EdgeId, VertexId};
use solidcore::{Brep, Kernel};

/// The default kernel with the three fixes under which the rollover failure shows.
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.offset_extension_from_base = true;
    k.fixes.fillet_refuse_overlapping_bands = true;
    k.fixes.offset_extension_as_itself = true;
    k
}

const X0: f64 = 41.0 + 1.0 / 6.0;
/// The arc's axis.
const CX: f64 = X0 + 21.0;

/// The arc part: counterclockwise (0, 0), (0, -21), (X0, -21), the arc about (CX, -21) up to
/// its tip (CX, 0), back along the top line; extruded 10 along z.
fn arc_part(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let mut arc = create_arc(Point3::new(CX, -21.0, 0.0), 21.0, 90.0, 180.0).unwrap();
    let ivl = arc.natural_interval();
    arc.reverse_parameterization(&ivl).unwrap();
    let curves = [
        Curve::from(create_line_segment(p(0.0, 0.0), p(0.0, -21.0)).unwrap()),
        Curve::from(create_line_segment(p(0.0, -21.0), p(X0, -21.0)).unwrap()),
        Curve::from(arc),
        Curve::from(create_line_segment(p(CX, 0.0), p(0.0, 0.0)).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// The test's picks: the long front edge, the arc's top edge, the back edge, the upright
/// between the front and the arc, the upright at the tip (the knife edge), the left top edge
/// and the left front upright.
const LONG: [f64; 3] = [20.0, -21.0, 10.0];
fn arc_top() -> [f64; 3] {
    let a = std::f64::consts::FRAC_PI_4;
    [CX - 21.0 * a.cos(), -21.0 + 21.0 * a.sin(), 10.0]
}
const BACK: [f64; 3] = [30.0, 0.0, 10.0];
const UPRIGHT: [f64; 3] = [X0, -21.0, 5.0];
const TIP: [f64; 3] = [CX, 0.0, 5.0];
const LEFT: [f64; 3] = [0.0, -10.0, 10.0];
const LEFT_FRONT: [f64; 3] = [0.0, -21.0, 5.0];

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> Option<EdgeId> {
    let p = Point3::new(at[0], at[1], at[2]);
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let Ok(d) = c.drop_point(
            k,
            &ivl,
            p,
            None,
            BIG_DOUBLE,
            None,
            SolverOperation::Minimize,
        ) else {
            continue;
        };
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.map(|(e, _)| e)
}

/// The unit normal of face `f` of `b` at (near) `p`.
fn normal_at(k: &Kernel, b: &Brep, f: solidcore::topology::FaceId, p: Point3) -> Option<Vec3> {
    let s = face::surface(b, f).ok()??;
    let uv = surface_closest_point(k, s, p).ok()?.uvs.first().copied()?;
    s.evaluate_normal(k, uv, Side::Left, Side::Left)
        .ok()?
        .unitize()
        .ok()
}

/// Whether edge `e`'s two faces meet tangentially at `p` (a smooth join, which a tangent
/// chain passes over: a round's own boundary, a knife edge).
fn smooth_at(k: &Kernel, b: &Brep, e: EdgeId, p: Point3) -> bool {
    let faces = edge::faces(b, e, None).unwrap_or_default();
    let (Some(&f), Some(&g)) = (faces.first(), faces.get(1)) else {
        return false;
    };
    match (normal_at(k, b, f, p), normal_at(k, b, g, p)) {
        (Some(n), Some(m)) => n.cross(m).length() < 0.02,
        _ => false,
    }
}

/// Edge `e`'s unit direction leaving vertex `v` (from a short chord at that end).
fn leaving(k: &Kernel, b: &Brep, e: EdgeId, v: VertexId) -> Option<Vec3> {
    let c = edge::curve(b, e).ok()??;
    let iv = edge::interval(b, e).ok()?;
    let vp = vertex::point(b, v).ok()?;
    let (a, z) = (
        c.evaluate_point(k, iv.min()).ok()?,
        c.evaluate_point(k, iv.max()).ok()?,
    );
    let (at, toward) = if a.distance(vp) <= z.distance(vp) {
        (iv.min(), iv.evaluate(1e-4))
    } else {
        (iv.max(), iv.evaluate(1.0 - 1e-4))
    };
    let p0 = c.evaluate_point(k, at).ok()?;
    let p1 = c.evaluate_point(k, toward).ok()?;
    (p1 - p0).unitize().ok()
}

/// The tangent chain of the edge at `at` (a fillet's default selection): from the
/// edge, at each end, the one edge that is no smooth join and leaves the vertex within 1 degree
/// of straight on, as far as it goes (by points halfway along each edge).
fn chain(k: &Kernel, b: &Brep, at: [f64; 3]) -> Vec<EdgeId> {
    let Some(start) = pick(k, b, at) else {
        return Vec::new();
    };
    let mut out = vec![start];
    for end in 0..2 {
        let mut cur = start;
        let mut from = if end == 0 {
            edge::start_vertex(b, cur).ok().flatten()
        } else {
            edge::end_vertex(b, cur).ok().flatten()
        };
        while let Some(v) = from {
            let Some(arrive) = leaving(k, b, cur, v).map(|d| -d) else {
                break;
            };
            let vp = vertex::point(b, v).unwrap();
            let mut next = None;
            for e in vertex::edges(b, v, None).unwrap_or_default() {
                if e == cur || out.contains(&e) || smooth_at(k, b, e, vp) {
                    continue;
                }
                if let Some(d) = leaving(k, b, e, v) {
                    if d.dot(arrive) > 1.0_f64.to_radians().cos() {
                        next = Some(e);
                    }
                }
            }
            let Some(n) = next else {
                break;
            };
            out.push(n);
            from = edge::other_vertex(b, n, Some(v)).ok().flatten();
            cur = n;
        }
    }
    out
}

/// What a round of `edges` at radius 2 does: the body (stitched, checked) or the session's
/// status and comment.
fn round(
    k: &mut Kernel,
    mut b: Brep,
    edges: &[EdgeId],
) -> Result<(Brep, Vec<String>), (i32, String, String)> {
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let radii = vec![2.0; edges.len()];
    let (failed, code, comment) = {
        let mut s = FilletSession::new(k, &mut b)
            .map_err(|e| (-1, String::new(), format!("session: {e}")))?;
        s.prepare_per_edge_constant_radius(k, edges, &radii, &xs, 1, 1.0)
            .map_err(|e| (-1, String::new(), format!("prepare: {e}")))?;
        let mut failed = s
            .create_corners(k, None)
            .err()
            .map(|e| format!("create_corners: {e:?}"));
        if failed.is_none() {
            failed = s
                .do_filleting(k, None)
                .err()
                .map(|e| format!("do_filleting: {e:?}"));
        }
        let info = s.error_info();
        (failed, info.code, info.comment.clone())
    };
    if let Some(err) = failed {
        return Err((code, comment, err));
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default())
        .map_err(|e| (-1, String::new(), format!("stitch: {e}")))?;
    let lines = check_brep(k, &mut b, CheckLevel::Level2)
        .map_err(|e| (-1, String::new(), format!("check: {e}")))?
        .lines();
    Ok((b, lines))
}

/// Every failure of a pair (the first round, the second on it, the part's own edges
/// of both together), as (which, code, comment, error).
fn failures(first: [f64; 3], then: [f64; 3]) -> Vec<(&'static str, i32, String, String)> {
    let mut out = Vec::new();
    let mut k = kernel();
    // (a fresh part for each round: a B-rep's clone does not keep its geometry's owners)
    let body = arc_part(&mut k);
    let part = arc_part(&mut k);
    let c1 = chain(&k, &part, first);
    let step = match round(&mut k, part, &c1) {
        Ok((b, _)) => b,
        Err((code, comment, err)) => {
            out.push(("first", code, comment, err));
            return out;
        }
    };
    let c2 = chain(&k, &step, then);
    let mut picks = vec![first, then];
    for &e in c2.iter().skip(1) {
        let c = edge::curve(&step, e).unwrap().unwrap();
        let iv = edge::interval(&step, e).unwrap();
        let m = c.evaluate_point(&k, iv.evaluate(0.5)).unwrap();
        let m = [m.x, m.y, m.z];
        if pick(&k, &body, m).is_some() {
            picks.push(m);
        }
    }
    if let Err((code, comment, err)) = round(&mut k, step, &c2) {
        out.push(("second", code, comment, err));
    }
    let mut together_edges: Vec<EdgeId> = Vec::new();
    for &p in &picks {
        for e in chain(&k, &body, p) {
            if !together_edges.contains(&e) {
                together_edges.push(e);
            }
        }
    }
    if let Err((code, comment, err)) = round(&mut k, body, &together_edges) {
        out.push(("together", code, comment, err));
    }
    out
}

fn pairs() -> Vec<([f64; 3], [f64; 3])> {
    vec![
        (LONG, LEFT),
        (LONG, LEFT_FRONT),
        (LONG, arc_top()),
        (arc_top(), LONG),
        (LONG, UPRIGHT),
        (arc_top(), UPRIGHT),
        (arc_top(), TIP),
        (BACK, TIP),
        (arc_top(), BACK),
        (BACK, arc_top()),
    ]
}

/// The default comment of an error info that noted nothing.
const NO_COMMENT: &str = "<no comment>\n";

#[test]
fn every_failure_notes_its_status() {
    for (first, then) in pairs() {
        for (which, code, comment, err) in failures(first, then) {
            println!("  {first:?} then {then:?}: {which} failed: status {code}: {comment} [{err}]");
            // (a stitch or check failure after the session is not the session's to note)
            let session = err.starts_with("create_corners") || err.starts_with("do_filleting");
            assert!(
                !session || (code > 0 && comment != NO_COMMENT),
                "{first:?} then {then:?}: the {which} round failed without noting why ({err})"
            );
        }
    }
}

/// With the three fixes the arc's round followed by the front edge's fails in the rollover and
/// notes it: `SM_FILERR_EDGE_PROBLEM` with what failed, not code 0.
#[test]
fn the_rollover_failure_notes_its_status() {
    let failed = failures(arc_top(), LONG);
    let (which, code, comment, _) = failed
        .first()
        .expect("the second round fails with the three fixes");
    assert_eq!(*which, "second");
    assert_eq!(*code, 4, "{comment}");
    assert!(
        comment.starts_with("Fillet Surface Computation Failure: "),
        "{comment}"
    );
}
