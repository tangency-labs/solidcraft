//! The largest gap of `topology.vertex` `surf_sphere150_rebuild_v3_gaps`
//! (`max_up_dim_gap_3d` with its list).
//!
//! The row reports the true largest gap to the rounding, PROVEN by the convex hull of the
//! net. The pole is not one point to the bit: rows 1 and 3 of the pole column are
//! `-5 + 2.8e-16` and `-5 - 1.4e-16` in `x`. Every face gap at the vertex has its `face_uv`
//! on a clamped boundary of the net (`v = 0`), where the surface is a convex combination of
//! the boundary's Cartesian control points (positive weights), so its true distance from
//! the vertex is at most their bound `delta` ([`hull_bound`], the rounding of computing it
//! charged); every edge gap is at its curve's clamped end, its end control point.
//! `delta <= 4e-16`, and every listed gap and the largest lie in `[0, delta]`: the
//! reported largest is the true largest gap, 0, to the rounding.

use std::collections::BTreeMap;

use solidcore::curve::curve::p2::CurveKind;
use solidcore::heal::gap::Gap;
use solidcore::math::Point3;
use solidcore::nurbs::math::{gamma, CPoint, NOW, NOZ};
use solidcore::topology::vertex::{self as vx, HeldGap, VertexGap};
use solidcore::topology::Brep;
use solidcore::Kernel;

use crate::case::{repo_root, Case, CaseContext};
use crate::UnitDiff;

/// The vertex row's case.
const CASE: &str = "surf_sphere150_rebuild_v3_gaps";

/// The `topology.vertex` unit and its cases, by name.
fn unit_cases() -> (Box<dyn UnitDiff>, BTreeMap<String, Case>) {
    let u = crate::registry()
        .into_iter()
        .find(|u| u.id() == "topology.vertex")
        .expect("the topology.vertex unit");
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = u
        .cases(&ctx)
        .expect("the cases")
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect();
    (u, cases)
}

/// A bound on `|X_j - V|` over the Cartesian points `X_j = x_j / w_j` of `points`
/// (homogeneous `x_j`, weight `w_j`): per coordinate the FMA `x_jc - V_c w_j` is the
/// exact difference rounded once, so the exact one is at most `|fma| / (1 - u) <=
/// |fma| (1 + 2u)`; their sum (the 1-norm, at least the Euclidean one) over `w_j`,
/// its two sums, the factor and the division charged `gamma(6)`.
///
/// # Panics
///
/// For a weight that is not positive.
fn hull_bound<'a>(points: impl IntoIterator<Item = &'a CPoint>, v: Point3) -> f64 {
    let u = f64::EPSILON / 2.0;
    let mut d = 0.0_f64;
    for c in points {
        let w = if c.w == NOW { 1.0 } else { c.w };
        assert!(w > 0.0, "a weight that is not positive: {c:?}");
        let z = if c.z == NOZ { 0.0 } else { c.z };
        let s = (-v.x).mul_add(w, c.x).abs()
            + (-v.y).mul_add(w, c.y).abs()
            + (-v.z).mul_add(w, z).abs();
        d = d.max(s * (1.0 + 2.0 * u) / w * (1.0 + gamma(6)));
    }
    d
}

/// Whether `knots` are clamped at `t` (`t` the first or last of `p + 1` equal knots).
fn clamped_end(knots: &[f64], p: usize, t: f64) -> Option<bool> {
    let n = knots.len();
    if knots.iter().take(p + 1).all(|&x| x == t) {
        Some(true)
    } else if n > p && knots[n - p - 1..].iter().all(|&x| x == t) {
        Some(false)
    } else {
        None
    }
}

/// The bound on the true gap of `held` (a gap at the vertex `v`): for a face gap, the
/// face's surface at `face_uv` lies on a clamped boundary of its net, so it is a convex
/// combination of that boundary's Cartesian control points (positive weights) and its
/// distance from `v` is at most their [`hull_bound`]; for an edge gap, the edge's curve
/// at `edge_t` is its clamped end, the end control point exactly.
///
/// # Panics
///
/// Where the gap is not at such a boundary or end.
fn true_gap_bound(k: &Kernel, b: &Brep, v: Point3, held: &HeldGap) -> f64 {
    match held.gap {
        VertexGap::Face(g) => {
            // (the cached copy's use: the holder where the copy has none)
            let g = solidcore::heal::gap::VertexFaceGap {
                vertexuse: g.vertexuse.or(Some(held.holder)),
                ..g
            };
            let face = Gap::from(g).face(b).expect("the face").expect("a face");
            let s = solidcore::topology::face::p2::surface(b, face)
                .expect("the surface")
                .expect("a surface");
            let form = solidcore::blend::fillet_section::station::as_b_spline(k, s)
                .expect("the NURBS form")
                .expect("a NURBS form");
            let nurb = form.gw_nurb().expect("a NURB");
            let net = nurb.cpts();
            let uv = g.face_uv;
            let (uk, vk) = (nurb.u_knots(), nurb.v_knots());
            let mut best = f64::INFINITY;
            if let Some(start) = clamped_end(vk, nurb.q(), uv.y) {
                let j = if start { 0 } else { net[0].len() - 1 };
                best = best.min(hull_bound(net.iter().map(|row| &row[j]), v));
            }
            if let Some(start) = clamped_end(uk, nurb.p(), uv.x) {
                let i = if start { 0 } else { net.len() - 1 };
                best = best.min(hull_bound(&net[i], v));
            }
            assert!(
                best.is_finite(),
                "face gap {held:?}: face_uv {uv:?} is on no clamped boundary"
            );
            best
        }
        VertexGap::Edge(g) => {
            let g = solidcore::heal::gap::VertexEdgeGap {
                vertexuse: g.vertexuse.or(Some(held.holder)),
                ..g
            };
            let edge = Gap::from(g).edge(b, 0).expect("the edge").expect("an edge");
            let curve = solidcore::topology::edge::curve(b, edge)
                .expect("the curve")
                .expect("a curve");
            let CurveKind::BSpline(bc) = curve.kind() else {
                panic!("edge gap {held:?}: not a B-spline curve: {curve:?}");
            };
            let nurb = bc.gw_nurb().expect("a NURB");
            let start = clamped_end(nurb.knots(), nurb.degree(), g.edge_t)
                .unwrap_or_else(|| panic!("edge gap {held:?}: edge_t not a clamped end"));
            let cp = nurb.cpts();
            hull_bound([if start { &cp[0] } else { &cp[cp.len() - 1] }], v)
        }
    }
}

/// Every listed gap and the reported largest of the vertex row lie within `delta <= 4e-16`
/// of 0, the bound on every true gap there.
#[test]
fn the_vertex_rows_largest_gap_is_the_true_largest() {
    let (_, all) = unit_cases();
    let (b, v, max, list) = super::wr57_gaps(&all[CASE]);
    let k = Kernel::default();
    let at = vx::point(&b, v).expect("the vertex");
    let max = max.expect("a largest gap");
    // (a gap the cache holds unmeasured, `-SM_UNDEF_DOUBLE`, has no point to bound)
    let unmeasured = |g: &HeldGap| g.gap.length() == -solidcore::math::UNDEF_DOUBLE;
    let list: Vec<HeldGap> = list.into_iter().filter(|g| !unmeasured(g)).collect();
    assert!(!list.is_empty(), "no measured gap");
    assert!(!unmeasured(&max), "the largest gap {max:?} is not measured");
    let mut delta = 0.0_f64;
    for g in &list {
        let bound = true_gap_bound(&k, &b, at, g);
        eprintln!("{g:?}: reported {:e}, true gap in [0, {bound:e}]", g.gap.length());
        delta = delta.max(bound);
    }
    eprintln!("vertex {at:?}, largest {max:?}, delta {delta:e}");
    assert!(delta <= 4e-16, "delta {delta:e}");
    for g in list.iter().chain([&max]) {
        let r = g.gap.length();
        assert!(
            (0.0..=delta).contains(&r),
            "the reported gap {r:e} of {g:?} is not in [0, {delta:e}]"
        );
    }
}

/// The row's record: the reported largest gap (an edge gap held by vertex use 2, every
/// listed gap of length 0) is one the list holds and no listed gap is longer, so the
/// row's answer is a maximum by construction.
#[test]
fn surf_sphere150_rebuild_v3_gaps_reports_a_largest_listed_gap() {
    let (u, all) = unit_cases();
    let out = u.run_rust(&all[CASE]).expect("the run");
    let s = &out.values["state"];
    // [status, max kind, max holder, max length, list length, (kind, holder, length)...]
    assert_eq!(s[0], 0.0);
    let max = (s[1], s[2], s[3]);
    let n = s[4] as usize;
    let list: Vec<(f64, f64, f64)> = (0..n)
        .map(|g| (s[5 + 3 * g], s[6 + 3 * g], s[7 + 3 * g]))
        .collect();
    assert_eq!(max, (0.0, 2.0, 0.0), "{s:?}");
    assert!(list.contains(&max), "{max:?} not in {list:?}");
    assert!(list.iter().all(|g| g.2 <= max.2), "{list:?}");
}
