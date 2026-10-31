//! The SolidCraft fork's tangency-labs/smlib@cd47efd, under WR-600's
//! `fillet_nx1_tangent_side_edge_same_convexity`: a G2 blend's end rib at a tangent end
//! is a B-spline in `t = sqrt(s)`, with zero speed there.
//!
//! The part is the fork's tangent-end peg (as in `fork_856b54e_tangent_end.rs`): a bar
//! 40 x 16 x 10 whose far end is a half disc R 8 about (40, 8), with a disc R 8 standing
//! 10 on its top about the same axis. Its front top edge gets a curvature-continuous
//! round of 2 (blend-curve section, G2: contact order k = 3), whose front rail crosses
//! the tangent seam x = 40 of the front face and the cylinder at P = (40, 0, 8). There
//! the end rib leaves P as `a ~ s^(3/2)` along the seam, its curvature growing as
//! `s^(-1/2)`. Before the change it was fitted in `s` (regular at P) and could not follow
//! that; now it is fitted in `t` with zero speed at P (its first two control points
//! coincide) and an exact second derivative there. The fork: "the end rib has zero speed
//! at P, its second derivative along T", "SMLib's check passes, and refuses the same
//! curve on no edge (tests 3 and 14)" (with WR-668's `curve_check_zero_speed_tangent_end`,
//! the fork's ecf1963).
//!
//! The fork's "volume 9393.307931047 against the quadrature's 9393.307936313" matches a
//! G2 section whose quintic control points are evenly spaced on the faces (2, 4/3, 2/3
//! from the edge; section area 0.597884, swept and cut at the cylinder as in
//! `fork_856b54e_tangent_end.rs`). solidcore's blend-curve section removes less here:
//! 9395.419893021, about the same with or without this change. That difference is not
//! the rib's, so the volume is checked against the same blend with the rib marched into
//! the tangent end.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn sweep(k: &mut Kernel, cs: &[Curve], h: f64) -> Brep {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), h, true).unwrap()
}

fn peg(k: &mut Kernel) -> Brep {
    let p = Point3::new;
    let line = |a: Point3, b: Point3| Curve::from(curves::create_line_segment(a, b).unwrap());
    let bar = [
        line(p(0.0, 0.0, 0.0), p(40.0, 0.0, 0.0)),
        Curve::from(curves::create_arc(p(40.0, 8.0, 0.0), 8.0, -90.0, 90.0).unwrap()),
        line(p(40.0, 16.0, 0.0), p(0.0, 16.0, 0.0)),
        line(p(0.0, 16.0, 0.0), p(0.0, 0.0, 0.0)),
    ];
    let bar = sweep(k, &bar, 10.0);
    let disc = [Curve::from(
        curves::create_circle(p(40.0, 8.0, 10.0), 8.0).unwrap(),
    )];
    let disc = sweep(k, &disc, 10.0);
    boolean_union(k, bar, disc).unwrap()
}

fn find_edges(b: &Brep, pred: impl Fn(Point3, Point3) -> bool) -> Vec<EdgeId> {
    get_edges(b)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            pred(
                b.vertex(s.unwrap()).unwrap().point,
                b.vertex(t.unwrap()).unwrap().point,
            )
        })
        .collect()
}

/// The front top edge, (0, 0, 10) to (40, 0, 10).
fn front_top(b: &Brep) -> EdgeId {
    let found = find_edges(b, |s, t| {
        (s.z - 10.0).abs() < 1e-9
            && (t.z - 10.0).abs() < 1e-9
            && s.y.abs() < 1e-9
            && t.y.abs() < 1e-9
            && (s.x - t.x).abs() > 30.0
    });
    assert_eq!(found.len(), 1);
    found[0]
}

/// The fork's profile around the change (as `fork_856b54e_tangent_end.rs`), with SMLib's
/// curve check reading the minimum radius (tests 13/14) and taking a zero-speed tangent
/// end (WR-668).
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_nx1_tangent_side_edge = true;
    k.fixes.fillet_nx1_opposite_continuation_side_edge = true;
    k.fixes.fillet_nx1_tangent_corner_edge = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_run_on_past_far_end_only = true;
    k.fixes.fillet_run_on_bspline_trims = true;
    k.fixes.fillet_chamfer_cone_on_arc = true;
    k.fixes.fillet_torus_span_from_start_section = true;
    k.fixes.cover_wraps_across_closed_seam = true;
    k.fixes.fillet_nx1_tangent_side_edge_same_convexity = true;
    k.fixes.bspline_check_min_radius = true;
    k.fixes.curve_check_zero_speed_tangent_end = true;
    k
}

#[test]
fn g2_front_round_end_rib_has_zero_speed_at_its_tangent_end() {
    let mut k = kernel();
    let mut b = peg(&mut k);
    let e = front_top(&b);
    fillet_edges(&mut k, &mut b, &[e], 2.0, 2, 2, 1.0).unwrap();

    // the end rib: the B-spline edge at P = (40, 0, 8) climbing the cylinder
    let p = Point3::new(40.0, 0.0, 8.0);
    let at_p = |q: Point3| q.distance(p) < 1e-6;
    let ribs: Vec<EdgeId> = find_edges(&b, |s, t| (at_p(s) || at_p(t)) && s.z.max(t.z) > 9.0)
        .into_iter()
        .filter(|&e| {
            matches!(
                b.edge(e).unwrap().curve.as_ref().map(Curve::kind),
                Some(CurveKind::BSpline(_))
            )
        })
        .collect();
    assert_eq!(ribs.len(), 1, "end ribs at P: {ribs:?}");
    let rib = b.edge(ribs[0]).unwrap().curve.clone().unwrap();
    let ivl = rib.natural_interval();
    let (t0, t1) = (ivl.min(), ivl.max());
    let t_p = if rib.evaluate_point(&k, t0).unwrap().distance(p) < 1e-6 {
        t0
    } else {
        t1
    };
    let d = rib.evaluate(&k, t_p, 2, t_p == t1, false).unwrap();
    // zero speed at P (the first two control points coincide), a non-zero second
    // derivative along the seam T (z)
    assert!(d[1].length() < 1e-9, "speed at P {:?}", d[1]);
    let d2 = d[2];
    assert!(d2.length() > 1e-3, "second derivative at P {d2:?}");
    let off_t = (d2.x * d2.x + d2.y * d2.y).sqrt() / d2.length();
    assert!(off_t < 1e-9, "second derivative off T: {d2:?}");

    // SMLib's check passes (tests 3 and 14 on the zero-speed end with WR-668)
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:#?}");

    let v = volume(&k, &mut b);
    // The rib is nearer the exact one than before but changes the volume little: the
    // same blend with its end rib marched into the tangent end (WR-600's flag off)
    // gives 9395.419872879.
    let mut k_off = kernel();
    k_off.fixes.fillet_nx1_tangent_side_edge_same_convexity = false;
    let mut b_off = peg(&mut k_off);
    let e_off = front_top(&b_off);
    fillet_edges(&mut k_off, &mut b_off, &[e_off], 2.0, 2, 2, 1.0).unwrap();
    let v_off = volume(&k_off, &mut b_off);
    assert!((v - v_off).abs() < 5e-5, "V {v} vs {v_off}");
    // 9395.419893021 here, 9395.419890495 with the rib fitted in s (before cd47efd)
    assert!((v - 9395.419893021).abs() < 1e-6, "V {v}");
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}
