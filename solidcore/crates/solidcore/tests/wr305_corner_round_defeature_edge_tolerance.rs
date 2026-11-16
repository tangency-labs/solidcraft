//! WR-305 (`classify_edge_interval`, retired as a flag): the goldens row
//! `blend.feature_executive` `single_face_fillet1019_6` (the corner round, face 6 of
//! `Fillet/fillet1019.smb`, taken off with the base's faces extended and the feature built
//! from the other B-rep) moved only in one edge's tolerance and its two vertices', in the
//! base and in the feature B-rep: 2.5778854416825133e-3 where the flag off left
//! 1.3684680561596358e-3. Every other value of the row (both graphs, every curve and
//! surface sample, every other tolerance) is the same.
//!
//! Two curved intersection pieces of the boolean are now classified on face edges, each by
//! the certificate (tangent to the edge at both ends, within the intersection tolerance of
//! it as a whole). The merged edge then has an edge use without a UV trim, and the stitch
//! fits one, whose fit error grows the edge tolerance. This test replays the row and proves
//! that edge's tolerance, old and new, covers the edge: the edge's curve lies within
//! [`cylinder_gap`] (a certified bound) of the cylinder each of its faces lies on (their
//! NURBS forms proved on it by [`AnalyticSupport::check`]), and that bound is below both.
//!
//! The model is the upstream test file, read by the Rust reader when the pinned upstream
//! checkout is present (as the goldens do).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::attribute::AttributeStore;
use solidcore::math::Vec3;
use solidcore::nurbs::math::gamma;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, edgeuse, vertex};
use solidcore::{Brep, Kernel};

/// The edge tolerance the row records now, and the one it recorded with the flag off.
const NEW: f64 = 0.002_577_885_441_682_513_3;
const OLD: f64 = 0.001_368_468_056_159_635_8;

/// `Fillet/fillet1019.smb`, read and arena-named as the goldens reader does.
fn fillet1019(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/fillet1019.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// A bound `G` with `| |X(t) - o - ((X(t) - o) . a) a| - r | <= G` at every parameter `t` of
/// `curve` (a non-rational B-spline over its whole natural interval): how far it is from
/// the cylinder about the axis `a` (unit) through `o` of radius `r`, every rounding counted.
///
/// The curve is cut into Bézier pieces, `per_span` per span of its knots (knot insertion,
/// then [`decompose_bez`](solidcore::nurbs::crv_basic::Curve::decompose_bez)). A piece lies
/// in the convex hull of its control points `P_i` (Piegl and Tiller, *The NURBS Book*,
/// 2nd ed., §5.6). The distance from the axis, `h(X) = |X - o - ((X - o) . a) a|`, is the
/// norm of a linear map of `X - o`, so it is convex: over the hull `h <= max h(P_i)`. For any
/// unit `n` perpendicular to `a`, `h(X) >= n . (X - o)` (Cauchy-Schwarz), a linear function,
/// so over the hull `h >= min n . (P_i - o)`; `n` is the direction of the centroid's
/// projection. The gap `|h - r|` is at most the larger of `max h(P_i) - r` and
/// `r - min n . (P_i - o)`. Rounding: each inserted knot moves a control point coordinate by
/// at most `γ_12` of twice the net's largest coordinate `M` (`nurbs::crv_deviation`'s module
/// docs, the same count of insertions), a point by at most twice that; `h` and `n . (P - o)`
/// are a handful of operations on numbers below `4 (M + |o| + r)` (with `n` and `a` unit and
/// perpendicular to a few ulps), rounded up by `γ_32` of twice that.
fn cylinder_gap(curve: &Curve, o: Vec3, a: Vec3, r: f64, per_span: usize) -> f64 {
    let CurveKind::BSpline(bs) = curve.kind() else {
        panic!("not a B-spline: {}", curve.type_code());
    };
    assert!(!curve.is_rational().unwrap(), "a rational curve");
    let nurb = bs.gw_nurb().expect("its NURB");
    let p = nurb.degree();
    let knots = nurb.knots().to_vec();
    let point = |c: &solidcore::nurbs::math::CPoint| Vec3::new(c.x, c.y, c.z);
    let m = nurb
        .cpts()
        .iter()
        .map(|c| c.x.abs().max(c.y.abs()).max(c.z.abs()))
        .fold(0.0, f64::max);
    // per_span - 1 knots inside each span of positive length
    let mut x = Vec::new();
    for w in knots.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if hi > lo {
            for j in 1..per_span {
                let u = lo + (hi - lo) * (j as f64) / (per_span as f64);
                assert!(u > lo && u < hi);
                x.push(u);
            }
        }
    }
    let refined = nurb.refine(&x).unwrap();
    let pieces = refined.decompose_bez().unwrap();
    // knots inserted: those of the pieces' joined knot vector, 2 (p + 1) + (s - 1) p, past
    // the curve's own (as `crv_deviation` counts them)
    let joined = (pieces.len() - 1) * p + 2 * (p + 1);
    let inserted = joined - knots.len();
    let coord = gamma((inserted + 1) * 12) * 2.0 * m;
    let moved = 2.0 * coord;
    let size = m + o.x.abs().max(o.y.abs()).max(o.z.abs()) + r;
    let round = gamma(32) * 8.0 * size;
    let perp = |x: Vec3| {
        let d = x - o;
        d - a * d.dot(a)
    };
    let mut worst: f64 = 0.0;
    for piece in &pieces {
        let pts: Vec<Vec3> = piece.cpts().iter().map(point).collect();
        let n = pts.len() as f64;
        let c = pts.iter().fold(Vec3::new(0.0, 0.0, 0.0), |s, &q| s + q) * (1.0 / n);
        let radial = perp(c);
        let unit = radial * (1.0 / radial.length());
        let h_max = pts.iter().map(|&q| perp(q).length()).fold(0.0, f64::max);
        let h_min = pts
            .iter()
            .map(|&q| unit.dot(q - o))
            .fold(f64::INFINITY, f64::min);
        let out = h_max - r;
        let inside = r - h_min;
        worst = worst.max(out.max(inside) + moved + round);
    }
    assert!(worst.is_finite());
    worst
}

/// The axis point, unit axis and radius of the cylinder `s`'s NURBS form is proved on.
fn cylinder_of(k: &Kernel, b: &Brep, f: solidcore::FaceId) -> (Vec3, Vec3, f64) {
    let s = b.face(f).unwrap().surface.as_ref().unwrap();
    match AnalyticSupport::check(k, s).unwrap() {
        AnalyticSupport::Cylinder { placement, radius }
        | AnalyticSupport::Cone {
            placement,
            base_radius: radius,
            semi_angle_deg: 0.0,
        } => (
            placement.origin(),
            placement.x_axis().cross(placement.y_axis()),
            radius,
        ),
        other => panic!("not a cylinder: {other:?}"),
    }
}

/// `b`'s one edge whose tolerance is `NEW`, with both its vertices at `NEW` too, proved
/// within a certified gap of the cylinders of its faces that lies below `OLD`: the old
/// tolerance and the new one both cover it. Returns that gap.
fn certify_moved_edge(k: &Kernel, b: &Brep, what: &str) -> f64 {
    let moved: Vec<_> = bp1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::tolerance(b, e).unwrap() == NEW)
        .collect();
    assert_eq!(moved.len(), 1, "{what}");
    let e = moved[0];
    let record = b.edge(e).unwrap();
    let ends = edge::vertices(b, e, None).unwrap();
    assert_eq!(ends.len(), 2, "{what}");
    for v in ends {
        assert_eq!(vertex::tolerance(b, v.unwrap()).unwrap(), NEW, "{what}");
    }
    let curve = record.curve.as_ref().unwrap();
    let domain = curve.natural_interval();
    assert_eq!(
        (record.interval.min(), record.interval.max()),
        (domain.min(), domain.max()),
        "{what}: the edge runs over its whole curve"
    );
    let mut gap: f64 = 0.0;
    let mut faces = 0;
    for &eu in &record.edgeuses {
        let f = edgeuse::p1::face(b, eu).unwrap().unwrap();
        let (o, a, r) = cylinder_of(k, b, f);
        gap = gap.max(cylinder_gap(curve, o, a, r, 64));
        faces += 1;
    }
    assert!(faces >= 2, "{what}");
    eprintln!("{what}: edge gap from its faces' cylinders at most {gap:e}");
    assert!(gap < OLD, "{what}: {gap:e}");
    gap
}

/// The goldens' `single_face_fillet1019_6` (inputs of `fixtures/blend.feature_executive`):
/// the two curved pieces go through the WR-305 certificate, and the one moved edge of the
/// base and of the feature B-rep is covered by its tolerance with and without the flag.
#[test]
fn corner_round_defeature_moves_only_a_covering_edge_tolerance() {
    let mut k = Kernel::default();
    let Some(mut b) = fillet1019(&mut k) else {
        return;
    };
    let face = bp1::faces(&b, None).unwrap()[6];
    let features = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        ex.add_feature(
            &mut k,
            &[face],
            RebuildBrepType::ExtendSurface,
            RebuildBrepType::FromOtherBrep,
        )
        .unwrap()
        .unwrap();
        let kept = ex.do_defeature(&mut k).unwrap();
        assert!(kept.is_empty(), "{kept:?}");
        ex.into_feature_breps()
    };
    assert_eq!(k.work_counters.curved_edge_runs(), 2);
    certify_moved_edge(&k, &b, "base");
    let feature: Vec<_> = features.into_iter().flatten().collect();
    assert_eq!(feature.len(), 1);
    certify_moved_edge(&k, &feature[0], "feature");
}

/// The bound is sound and tight on a curve whose distance from a cylinder is known: a
/// segment along a surface line of the cylinder of radius 5 about the z axis, and the same
/// segment 1e-4 off it. The slack (1e-11) is the counted rounding of the bound itself.
#[test]
fn cylinder_gap_bounds_a_known_distance() {
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    let (o, a) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let segment = |x: f64| {
        Curve::from(
            BSplineCurve::create_line_segment(3, Vec3::new(x, 0.0, 0.0), Vec3::new(x, 0.0, 10.0))
                .unwrap(),
        )
    };
    let on = cylinder_gap(&segment(5.0), o, a, 5.0, 16);
    assert!(on < 1e-11, "{on:e}");
    let off = cylinder_gap(&segment(5.0001), o, a, 5.0, 16);
    assert!((5.0001 - 5.0..1e-4 + 1e-11).contains(&off), "{off:e}");
}
