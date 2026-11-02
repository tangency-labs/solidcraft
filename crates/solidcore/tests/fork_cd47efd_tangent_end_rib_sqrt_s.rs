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
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_section::section::{GnSectionFsg, GN_THUMBWEIGHT_EQUAL_AREA};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
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

/// The fork's 9393.307931047 (tangency-labs/smlib `cd47efd`) is the G2 round with the
/// blend curve's quintic control points at thirds (the fork-only `SetBlendCurveAtThirds`,
/// `fd3b6a7`, ported as `FilletSession::set_blend_curve_at_thirds`, #1438), which
/// `fillet_edges` never turns on. Built as `fillet_edges` does internally, with the mode
/// on, solidcore measures 9393.307938954; the off path stays pinned above (9395.419893021).
#[test]
fn g2_front_round_at_thirds_matches_the_fork_volume() {
    let mut k = kernel();
    let mut b = peg(&mut k);
    let e = front_top(&b);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.set_blend_curve_at_thirds(true);
        s.prepare_constant_radius_edges(&mut k, &[e], 2.0, 2, 2, 1.0)
            .unwrap();
        s.do_fillet(&mut k).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let v = volume(&k, &mut b);
    assert!((v - 9393.307931047).abs() < 1e-5, "V {v}");
}

/// Quadrature volume (composite Simpson's rule) of the peg after cutting the order-`n` section
/// `(y(t), d(t))` of radius 2 and thumbweight `w` along the front top edge up to the cylinder
/// `(40 - x)^2 + (8 - y)^2 = 64`.
fn front_gn_quadrature_volume(n: usize, w: f64) -> f64 {
    let peg_v = 6400.0 + 96.0 * core::f64::consts::PI * 10.0;
    let p_exp = i32::try_from(n + 1).unwrap();
    let integrand = |t: f64| -> f64 {
        let h = 1e-7;
        let pt = |u: f64| -> (f64, f64) {
            let (a, b) = ((1.0 - u).powi(p_exp), u.powi(p_exp));
            let f = a / (a + b);
            let p1 = u / (1.0 - w + w * u);
            let p2 = u * (1.0 - w) / (1.0 - w * u);
            ((1.0 - f) * p2 * 2.0, f * (1.0 - p1) * 2.0)
        };
        let (y, d) = pt(t);
        let dy = if t <= h {
            (pt(t + h).0 - y) / h
        } else if t >= 1.0 - h {
            (y - pt(t - h).0) / h
        } else {
            (pt(t + h).0 - pt(t - h).0) / (2.0 * h)
        };
        let len_x = 40.0 - (16.0 * y - y * y).max(0.0).sqrt();
        d * len_x * dy
    };
    let m = 10_000; // even
    let mut s = integrand(0.0) + integrand(1.0);
    for i in 1..m {
        let w_s = if i % 2 == 1 { 4.0 } else { 2.0 };
        s += w_s * integrand(f64::from(i) / f64::from(m));
    }
    let removed = s / (3.0 * f64::from(m));
    peg_v - removed
}

/// smlib#78 (`7268cee`, #2250): with `fillet_gn_section_generator` on, between the peg's two
/// perpendicular planes `gn_section_contact_order` gives `k = n + 2`:
/// - at `n = 2` (`k = 4`, even), the end rib at `P = (40, 0, 8)` is regular in `s` along `T`;
/// - at `n = 3` (`k = 5`, odd), the end rib is fitted in `t = sqrt(s)` with zero speed at `P`
///   and second derivative along `T`.
///
/// At both `GN_THUMBWEIGHT_EQUAL_AREA` and `w = 0`, all pass `Level2` and match Simpson
/// quadrature of their section volumes.
#[test]
fn g2_and_g3_front_round_gn_section_end_rib_and_quadrature_volumes() {
    let w_eq2 = GnSectionFsg::equal_area_thumbweight(2, core::f64::consts::FRAC_PI_2).unwrap();
    let w_eq3 = GnSectionFsg::equal_area_thumbweight(3, core::f64::consts::FRAC_PI_2).unwrap();
    for (cont, w_arg, w_eff, zero_speed) in [
        (2u32, GN_THUMBWEIGHT_EQUAL_AREA, w_eq2, false),
        (2u32, 0.0, 0.0, false),
        (3u32, GN_THUMBWEIGHT_EQUAL_AREA, w_eq3, true),
        (3u32, 0.0, 0.0, true),
    ] {
        let mut k = kernel();
        k.fixes.fillet_gn_section_generator = true;
        let mut b = peg(&mut k);
        let e = front_top(&b);
        fillet_edges(&mut k, &mut b, &[e], 2.0, 2, cont, w_arg).unwrap();

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
        assert_eq!(ribs.len(), 1, "n={cont} w={w_arg}: end ribs at P: {ribs:?}");
        let rib = b.edge(ribs[0]).unwrap().curve.clone().unwrap();
        let ivl = rib.natural_interval();
        let (t0, t1) = (ivl.min(), ivl.max());
        let t_p = if rib.evaluate_point(&k, t0).unwrap().distance(p) < 1e-6 {
            t0
        } else {
            t1
        };
        let d = rib.evaluate(&k, t_p, 2, t_p == t1, false).unwrap();
        if zero_speed {
            assert!(
                d[1].length() < 1e-9,
                "n={cont} w={w_arg}: speed at P {:?}",
                d[1]
            );
            let d2 = d[2];
            assert!(
                d2.length() > 1e-3,
                "n={cont} w={w_arg}: second derivative at P {d2:?}"
            );
            let off_t = (d2.x * d2.x + d2.y * d2.y).sqrt() / d2.length();
            assert!(
                off_t < 1e-9,
                "n={cont} w={w_arg}: second derivative off T: {d2:?}"
            );
        } else {
            let d1 = d[1];
            assert!(d1.length() > 1.0, "n={cont} w={w_arg}: speed at P {d1:?}");
            let off_t = d1.x.hypot(d1.y) / d1.length();
            assert!(off_t < 1e-2, "n={cont} w={w_arg}: tangent off T: {d1:?}");
        }

        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "n={cont} w={w_arg}: {lines:#?}");

        let v = volume(&k, &mut b);
        let want = front_gn_quadrature_volume(usize::try_from(cont).unwrap(), w_eff);
        assert!(
            (v - want).abs() < 2e-5,
            "n={cont} w={w_arg}: V {v:.9} vs quadrature {want:.9}"
        );
    }
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}
