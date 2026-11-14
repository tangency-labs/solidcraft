//! A general-path sweep is a `ProceduralSweep` definition with a certified bound (#2572,
//! slice 2; not upstream): the profile carried along the path in the path's
//! rotation-minimizing frame, in closed form, beside the net `sweep_scale` fits and the
//! net's certified distance from it, `|B - S| <= bound`.
//!
//! - A circle along a line, an arc, or a line and then an arc: the definition's points are
//!   the exact cylinder, torus or both, at off-net parameters, and the net lies within the
//!   bound of them.
//! - A circle along a 3D spline of straights and R2 rounds (a battery pack's balance wire,
//!   a 1.5 mm circle swept along a fitted spline): the wall reports its definition and its
//!   bound; the definition's points are the circle about the path's point at the net's
//!   parameter, and the net lies within the bound of them at parameters off its knots.
//! - The definition follows its net's edits.
//! - Every wall face is a definition, or of a kind `data/sweep_definition_ledger/` lists.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::primitive::api_curves::create_arc;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::surface::procedural_sweep::{ProceduralSweep, SweepRef};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::api_brep::{create_curve_sweep, create_pipe_sweep};
use solidcore::topology::brep::p1;
use solidcore::topology::face;
use solidcore::{Brep, Kernel};

use std::collections::{BTreeMap, BTreeSet};

/// The balance wire's fit points: straights joined by R2 rounds, in mm.
const FIT: &[(f64, f64, f64)] = &[
    (128.855, 22.2, 63.309),
    (130.937, 22.2, 63.309),
    (133.018, 22.2, 63.309),
    (135.1, 22.2, 63.309),
    (135.711, 22.2, 63.253),
    (136.211, 22.2, 63.087),
    (136.6, 22.2, 62.809),
    (136.878, 22.2, 62.42),
    (137.044, 22.2, 61.92),
    (137.1, 22.2, 61.309),
    (137.1, 22.2, 59.1),
    (137.1, 22.144, 58.489),
    (137.1, 21.978, 57.989),
    (137.1, 21.7, 57.6),
    (137.1, 21.311, 57.322),
    (137.1, 20.811, 57.156),
    (137.1, 20.2, 57.1),
    (137.1, 17.247, 57.1),
    (137.1, 14.295, 57.1),
    (137.1, 11.342, 57.1),
    (137.1, 8.389, 57.1),
    (137.1, 5.437, 57.1),
    (137.1, 2.484, 57.1),
    (137.1, -0.468, 57.1),
    (137.1, -3.421, 57.1),
    (137.1, -6.374, 57.1),
    (137.1, -9.326, 57.1),
    (137.1, -12.279, 57.1),
    (137.1, -15.232, 57.1),
    (137.1, -18.184, 57.1),
    (137.1, -21.137, 57.1),
    (137.1, -24.089, 57.1),
    (137.1, -27.042, 57.1),
    (137.1, -29.995, 57.1),
    (137.1, -32.947, 57.1),
    (137.1, -35.9, 57.1),
    (137.1, -36.511, 57.044),
    (137.1, -37.011, 56.878),
    (137.1, -37.4, 56.6),
    (137.1, -37.678, 56.211),
    (137.1, -37.844, 55.711),
    (137.1, -37.9, 55.1),
    (137.1, -37.9, 52.55),
    (137.156, -37.9, 51.939),
    (137.322, -37.9, 51.439),
    (137.6, -37.9, 51.05),
    (137.989, -37.9, 50.772),
    (138.489, -37.9, 50.606),
    (139.1, -37.9, 50.55),
    (142.055, -37.9, 50.55),
    (145.011, -37.9, 50.55),
    (147.966, -37.9, 50.55),
    (150.922, -37.9, 50.55),
    (153.877, -37.9, 50.55),
    (156.833, -37.9, 50.55),
    (159.788, -37.9, 50.55),
    (162.744, -37.9, 50.55),
    (165.7, -37.9, 50.55),
    (168.655, -37.9, 50.55),
    (168.67, -37.9, 48.519),
    (168.685, -37.9, 46.489),
];

/// The pipe's radius: the wire's 1.5 mm circle.
const RADIUS: f64 = 0.75;

/// The balance wire's points fitted by a cubic spline closed (C1 between its two ends), as
/// the case was written.
fn spline_path() -> Curve {
    let pts: Vec<Point3> = FIT.iter().map(|&(x, y, z)| Point3::new(x, y, z)).collect();
    Curve::from(
        BSplineCurve::interpolate_points(&pts, None, 3, None, None, true, ParamMethod::ChordLength)
            .unwrap(),
    )
}

fn line_path() -> Curve {
    Curve::from(
        BSplineCurve::create_line_segment(
            3,
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(4.0, 6.0, 15.0),
        )
        .unwrap(),
    )
}

fn arc_path() -> Curve {
    Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 90.0).unwrap())
}

/// The options `create_pipe_sweep` sweeps with: the circle centred on the path, square to it.
fn pipe_options() -> SweepOptions {
    SweepOptions {
        move_profile_center_to_path: true,
        orient_profile_perpendicular_to_path: true,
        ..SweepOptions::default()
    }
}

/// The disc square to the start of the line-then-arc path, and the path: ten along x, then
/// a quarter arc of radius 10 turning to +y.
fn line_then_arc(k: &Kernel) -> (Curve, Curve) {
    let disc = Axis2Placement::new(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let circle =
        Curve::from(Ellipse::create_canonical_circle(k, &disc, RADIUS, None, None).unwrap());
    let line =
        Line::create_line_segment(3, Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), None)
            .unwrap();
    let arc_frame = Axis2Placement::new(
        Vec3::new(10.0, 10.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    );
    let arc = Ellipse::create_canonical_circle(
        k,
        &arc_frame,
        10.0,
        Some(&Extent1d::from_min_max(0.0, 90.0)),
        None,
    )
    .unwrap();
    let path = solidcore::curve::composite_curve::CompositeCurve::build_composites_from_curves(
        k,
        vec![Curve::from(line), Curve::from(arc)],
        1e-6,
        0.0,
    )
    .unwrap()
    .composites
    .into_iter()
    .next()
    .unwrap()
    .make_composite_nurb()
    .unwrap();
    (circle, Curve::from(path))
}

fn walls(b: &Brep) -> Vec<Surface> {
    p1::faces(b, None)
        .unwrap()
        .into_iter()
        .map(|f| face::p2::surface(b, f).unwrap().unwrap().clone())
        .collect()
}

/// The farthest the net strays from the definition over a grid of the face's `(u, v)` off
/// the net's knots, the net's own parameter domain (a long axis sampled densely), and the
/// definition's.
fn deviation(s: &SweepRef<'_>, along: usize) -> f64 {
    let dom = s.net().natural_uv_domain();
    let (u, v) = (dom.u_interval(), dom.v_interval());
    let (nu, nv) = if s.definition().definition_map().2 {
        (along, 24)
    } else {
        (24, along)
    };
    let mut far = 0.0_f64;
    for i in 0..nu {
        for j in 0..nv {
            let uv = Point2::new(
                u.min() + (u.max() - u.min()) * (f64::from(i as u32) + 0.37) / f64::from(nu as u32),
                v.min() + (v.max() - v.min()) * (f64::from(j as u32) + 0.61) / f64::from(nv as u32),
            );
            let gap = (s.net().evaluate_point(uv).unwrap()
                - s.definition().evaluate_definition_point(uv).unwrap())
            .length();
            far = far.max(gap);
        }
    }
    far
}

/// Every wall of `b` is a definition of a path motion; returns the first.
fn only_wall(b: &Brep) -> Surface {
    let w = walls(b);
    assert_eq!(w.len(), 1);
    assert!(
        w[0].procedural_sweep().is_some(),
        "the wall keeps its definition"
    );
    w.into_iter().next().unwrap()
}

/// The definition's point at `uv` is `radius` from the path's point at the net's
/// parameter `v`: the circle about the path.
fn on_tube(d: &ProceduralSweep, uv: Point2) -> f64 {
    let m = d.path().unwrap();
    let wt = d.definition_parameters(uv);
    let (t, _) = m.path_parameter(wt.y);
    let on_path = m.path().evaluate_point(t).unwrap();
    (d.evaluate_definition_point(uv).unwrap() - on_path).length()
}

/// A circle along a line is the exact cylinder: the definition's points are `RADIUS` from
/// the axis and between the ends, the bound is the rounding alone, and the net is within it.
#[test]
fn a_circle_along_a_line_is_the_exact_cylinder() {
    let mut k = Kernel::default();
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&line_path()), false).unwrap();
    let wall = only_wall(&b);
    let sweep = wall.procedural_sweep().unwrap();
    assert!(sweep.bound() < 1e-9, "bound {}", sweep.bound());
    let (a, e) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 6.0, 15.0));
    let axis = (e - a) * (1.0 / (e - a).length());
    let dom = sweep.net().natural_uv_domain();
    for i in 0..37 {
        for j in 0..29 {
            let uv = Point2::new(
                dom.u_interval().min()
                    + (dom.u_interval().max() - dom.u_interval().min()) * (f64::from(i) + 0.3)
                        / 37.0,
                dom.v_interval().min()
                    + (dom.v_interval().max() - dom.v_interval().min()) * (f64::from(j) + 0.4)
                        / 29.0,
            );
            let p = sweep.definition().evaluate_definition_point(uv).unwrap();
            let rel = p - a;
            let along = rel.dot(axis);
            let radial = (rel - axis * along).length();
            assert!((radial - RADIUS).abs() <= 1e-12, "{radial}");
            assert!(
                along > -1e-12 && along < (e - a).length() + 1e-12,
                "{along}"
            );
        }
    }
    assert!(deviation(&sweep, 29) <= sweep.bound());
}

/// A circle along an arc is the exact torus: the definition's points are `RADIUS` from the
/// arc's circle, the net is within the certified bound, and the bound is no looser than a
/// few times the net's true distance.
#[test]
fn a_circle_along_an_arc_is_the_exact_torus() {
    let mut k = Kernel::default();
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&arc_path()), false).unwrap();
    let wall = only_wall(&b);
    let sweep = wall.procedural_sweep().unwrap();
    let dom = sweep.net().natural_uv_domain();
    for i in 0..61 {
        for j in 0..23 {
            let uv = Point2::new(
                dom.u_interval().min()
                    + (dom.u_interval().max() - dom.u_interval().min()) * (f64::from(i) + 0.3)
                        / 61.0,
                dom.v_interval().min()
                    + (dom.v_interval().max() - dom.v_interval().min()) * (f64::from(j) + 0.4)
                        / 23.0,
            );
            let p = sweep.definition().evaluate_definition_point(uv).unwrap();
            let ring = p.x.hypot(p.y);
            let minor = (ring - 10.0).hypot(p.z);
            assert!((minor - RADIUS).abs() <= 1e-12, "{minor}");
            assert!(p.x >= -1e-12 && p.y >= -1e-12, "{p:?}");
        }
    }
    let far = deviation(&sweep, 2001);
    assert!(far <= sweep.bound(), "{far} past {}", sweep.bound());
    // the net is the definition to rounding: the bound is the enclosure's, well within the
    // kernel's tolerance, not a multiple of the (rounding-sized) distance
    assert!(
        sweep.bound() <= solidcore::geom::tol::ZONE_TOL_3D / 10.0,
        "{}",
        sweep.bound()
    );
}

/// A circle along a straight and then an arc: two pieces joined, one wall, whose definition
/// carries the piece's parameters.
#[test]
fn a_circle_along_a_line_then_an_arc_has_one_definition() {
    let mut k = Kernel::default();
    let (circle, path) = line_then_arc(&k);
    let sweep = create_curve_sweep(
        &mut k,
        &[Some(&circle)],
        Some(&path),
        None,
        None,
        Some(&pipe_options()),
    )
    .unwrap();
    let all = walls(&sweep.brep);
    assert!(!all.is_empty());
    for wall in &all {
        let s = wall.procedural_sweep().expect("a definition");
        let far = deviation(&s, 3001);
        assert!(far <= s.bound(), "{far} past {}", s.bound());
        let dom = s.net().natural_uv_domain();
        for i in 0..40 {
            for j in 0..12 {
                let uv = Point2::new(
                    dom.u_interval().min()
                        + (dom.u_interval().max() - dom.u_interval().min()) * (f64::from(i) + 0.3)
                            / 40.0,
                    dom.v_interval().min()
                        + (dom.v_interval().max() - dom.v_interval().min()) * (f64::from(j) + 0.4)
                            / 12.0,
                );
                let gap = on_tube(s.definition(), uv);
                assert!((gap - RADIUS).abs() <= 1e-9, "{gap}");
            }
        }
    }
}

/// A battery pack's balance wire path: straights joined by R2 rounds (lines and circular
/// arcs, G1), in mm.
fn wire_path(k: &Kernel) -> BSplineCurve {
    let p = |x: f64, y: f64, z: f64| Point3::new(x, y, z);
    let line =
        |a: Point3, b: Point3| Curve::from(BSplineCurve::create_line_segment(3, a, b).unwrap());
    let arc = |c: Point3, a: Point3, b: Point3| {
        Curve::from(
            BSplineCurve::create_arc_from_points(c, a, b, NurbCircleParam::Quadratic).unwrap(),
        )
    };
    let pieces = vec![
        line(p(128.855, 22.2, 63.309), p(135.1, 22.2, 63.309)),
        arc(
            p(135.1, 22.2, 61.309),
            p(135.1, 22.2, 63.309),
            p(137.1, 22.2, 61.309),
        ),
        line(p(137.1, 22.2, 61.309), p(137.1, 22.2, 59.1)),
        arc(
            p(137.1, 20.2, 59.1),
            p(137.1, 22.2, 59.1),
            p(137.1, 20.2, 57.1),
        ),
        line(p(137.1, 20.2, 57.1), p(137.1, -35.9, 57.1)),
        arc(
            p(137.1, -35.9, 55.1),
            p(137.1, -35.9, 57.1),
            p(137.1, -37.9, 55.1),
        ),
        line(p(137.1, -37.9, 55.1), p(137.1, -37.9, 52.55)),
    ];
    solidcore::curve::composite_curve::CompositeCurve::build_composites_from_curves(
        k, pieces, 1e-6, 0.0,
    )
    .unwrap()
    .composites
    .into_iter()
    .next()
    .unwrap()
    .make_composite_nurb()
    .unwrap()
}

/// The path with each of its knot spans split into `parts` (same curve, same parameters).
fn sampled(path: &BSplineCurve, parts: u32) -> BSplineCurve {
    let mut p = path.clone();
    let knots = path.canonical().unwrap().knots;
    let extra: Vec<f64> = knots
        .windows(2)
        .flat_map(|w| {
            (1..parts).map(move |i| w[0] + (w[1] - w[0]) * f64::from(i) / f64::from(parts))
        })
        .collect();
    p.insert_knots(&extra).unwrap();
    p
}

/// The balance wire (straights and R2 rounds): every wall is a definition, the
/// definition's points are the circle of `RADIUS` about the path's point, and the net lies
/// within the bound of them off its knots, whatever number of points the path is given.
#[test]
fn a_circle_sweeps_along_a_wire_of_straights_and_rounds() {
    let mut k = Kernel::default();
    let wire = wire_path(&k);
    for parts in [1, 2, 4] {
        let path = Curve::from(sampled(&wire, parts));
        let b = create_pipe_sweep(&mut k, RADIUS, Some(&path), false).unwrap();
        let all = walls(&b);
        assert!(!all.is_empty());
        for wall in &all {
            let sweep = wall.procedural_sweep().expect("every wall is a definition");
            let d = sweep.definition();
            assert!(d.path().is_some() && d.screw().is_none());
            assert!(sweep.bound().is_finite() && sweep.bound() > 0.0);
            let dom = sweep.net().natural_uv_domain();
            for i in 0..30 {
                for j in 0..200 {
                    let uv = Point2::new(
                        dom.u_interval().min()
                            + (dom.u_interval().max() - dom.u_interval().min())
                                * (f64::from(i) + 0.3)
                                / 30.0,
                        dom.v_interval().min()
                            + (dom.v_interval().max() - dom.v_interval().min())
                                * (f64::from(j) + 0.4)
                                / 200.0,
                    );
                    assert!((on_tube(d, uv) - RADIUS).abs() <= 1e-9);
                }
            }
            let far = deviation(&sweep, 3001);
            // the net is built from the definition: its bound is within the kernel's tolerance
            assert!(
                sweep.bound() <= solidcore::geom::tol::ZONE_TOL_3D,
                "{parts}: bound {}",
                sweep.bound()
            );
            assert!(
                far <= sweep.bound(),
                "{parts}: {far} past {}",
                sweep.bound()
            );
        }
    }
}

/// The definition's frame is the path's: the same at a parameter whether the path is
/// sampled once, twice or four times as densely.
#[test]
fn the_frame_does_not_depend_on_how_the_path_is_sampled() {
    let mut k = Kernel::default();
    let wire = wire_path(&k);
    let mut motion = |parts: u32| {
        let path = Curve::from(sampled(&wire, parts));
        let b = create_pipe_sweep(&mut k, RADIUS, Some(&path), false).unwrap();
        let wall = walls(&b).into_iter().next().unwrap();
        let sweep = wall.procedural_sweep().unwrap();
        sweep.definition().path().unwrap().clone()
    };
    let (m1, m2, m4) = (motion(1), motion(2), motion(4));
    assert_eq!(m2.spans().len(), 2 * m1.spans().len());
    assert_eq!(m4.spans().len(), 4 * m1.spans().len());
    let ivl = m1.path().natural_interval();
    for i in 0..400 {
        let t = ivl.min() + (ivl.max() - ivl.min()) * (f64::from(i) + 0.37) / 400.0;
        let f1 = m1.frame(t).unwrap();
        for m in [&m2, &m4] {
            let f = m.frame(t).unwrap();
            for (a, b) in f1.axes.iter().zip(&f.axes) {
                assert!((*a - *b).length() <= 1e-9, "t = {t}");
            }
        }
    }
}

/// Across every joint of the wire the frame is continuous; along a straight it does not
/// twist, and a round turns it with the tangent and no more.
#[test]
fn the_frame_is_continuous_across_joints_and_does_not_twist_on_a_straight() {
    let mut k = Kernel::default();
    let wire = wire_path(&k);
    let path = Curve::from(wire);
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&path), false).unwrap();
    let wall = walls(&b).into_iter().next().unwrap();
    let m = wall
        .procedural_sweep()
        .unwrap()
        .definition()
        .path()
        .unwrap()
        .clone();
    assert_eq!(m.spans().len(), 7);
    for pair in m.spans().windows(2) {
        let t = pair[0].to;
        let (a, b) = (m.frame(t - 1e-9).unwrap(), m.frame(t + 1e-9).unwrap());
        for (u, v) in a.axes.iter().zip(&b.axes) {
            assert!((*u - *v).length() <= 1e-6, "joint at {t}");
        }
    }
    for s in m.spans() {
        let (f0, f1) = (
            m.frame(s.from + 1e-9).unwrap(),
            m.frame(s.to - 1e-9).unwrap(),
        );
        match s.kind {
            solidcore::surface::procedural_sweep::path::SpanKind::Line => {
                assert!(
                    (f0.axes[2] - f1.axes[2]).length() <= 1e-6,
                    "twist on a straight"
                );
                assert!(f0.rates[2].length() <= 1e-12);
            }
            solidcore::surface::procedural_sweep::path::SpanKind::Arc { axis, .. } => {
                // no twist about the tangent: the normal's turn is about the arc's axis
                assert!(f0.rates[2].dot(f0.axes[1]).abs() <= 1e-9);
                assert!((f0.axes[2].dot(axis) - f1.axes[2].dot(axis)).abs() <= 1e-9);
            }
        }
    }
}

/// The definition follows its net: a rigid motion moves both, a reversal, a swap and a
/// reparametrisation move its map, a piece keeps it.
#[test]
fn the_path_definition_follows_its_nets_edits() {
    let mut k = Kernel::default();
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&arc_path()), false).unwrap();
    let wall = only_wall(&b);
    let base = deviation(&wall.procedural_sweep().unwrap(), 801);
    assert!(base <= wall.procedural_sweep().unwrap().bound());
    let place = Axis2Placement::new(
        Point3::new(10.0, -3.0, 7.0),
        Vec3::new(0.6, 0.8, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    );
    let mut moved = wall.copy(&k).unwrap();
    moved.transform(&k, &place, None).unwrap();
    let s = moved.procedural_sweep().expect("moved with its net");
    let far = deviation(&s, 801);
    assert!(far <= s.bound(), "{far} past {}", s.bound());
    assert!((far - base).abs() <= 1e-6 * (1.0 + base));
    for edit in 0..4 {
        let mut e = wall.copy(&k).unwrap();
        match edit {
            0 => e.reverse(&k, SurfParamType::U).unwrap(),
            1 => e.reverse(&k, SurfParamType::V).unwrap(),
            2 => e.swap_uv(&k).unwrap(),
            _ => {
                let dom = solidcore::geom::extent2d::Extent2d::from_bounds(-1.0, 3.0, 2.0, 9.0);
                e.reparameterize(&k, &dom).unwrap();
            }
        }
        let s = e.procedural_sweep().expect("edited with its net");
        let far = deviation(&s, 801);
        assert!(far <= s.bound(), "edit {edit}: {far} past {}", s.bound());
    }
    // a scale that is not uniform leaves no definition, and the net moves without it
    let mut squashed = wall.copy(&k).unwrap();
    squashed
        .transform(
            &k,
            &Axis2Placement::default(),
            Some(Vec3::new(1.0, 1.0, 2.0)),
        )
        .unwrap();
    assert!(squashed.procedural_sweep().is_none());
    assert!(matches!(squashed.kind(), SurfaceKind::BSpline(_)));
}

/// The ledger: each kind of wall face that is not a definition yet, with its issue (the
/// first `#n` of its file); only the `path-` kinds are this slice's.
fn path_ledger() -> BTreeMap<String, u32> {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/sweep_definition_ledger"
    );
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if !stem.starts_with("path-") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let issue = text
            .split('#')
            .nth(1)
            .and_then(|t| {
                t.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .unwrap_or_else(|| panic!("{stem}: the ledger entry names no issue"));
        out.insert(stem, issue);
    }
    out
}

/// Every wall face of every path sweep case is a definition, or of a kind the ledger lists;
/// and every listed kind still appears.
#[test]
fn issue_2572_every_path_wall_is_a_definition_or_in_the_ledger() {
    let mut k = Kernel::default();
    let (circle, path) = line_then_arc(&k);
    let translational = SweepOptions {
        translational_sweep: true,
        ..pipe_options()
    };
    let reference = Curve::from(
        Line::create_line_segment(3, Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), None)
            .unwrap(),
    );
    let scale = Curve::from(
        Line::create_line_segment(3, Vec3::new(0.0, 1.0, 0.0), Vec3::new(10.0, 2.0, 0.0), None)
            .unwrap(),
    );
    let wire = Curve::from(wire_path(&k));
    let closed = Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 360.0).unwrap());
    // (the kind its bare walls are listed as, the body)
    let cases: Vec<(&str, Brep)> = vec![
        (
            "path-b-spline",
            create_pipe_sweep(&mut k, RADIUS, Some(&line_path()), false).unwrap(),
        ),
        (
            "path-b-spline",
            create_pipe_sweep(&mut k, RADIUS, Some(&arc_path()), false).unwrap(),
        ),
        (
            "path-b-spline",
            create_pipe_sweep(&mut k, RADIUS, Some(&wire), false).unwrap(),
        ),
        (
            "path-b-spline",
            create_curve_sweep(
                &mut k,
                &[Some(&circle)],
                Some(&path),
                None,
                None,
                Some(&pipe_options()),
            )
            .unwrap()
            .brep,
        ),
        (
            "path-translational",
            create_curve_sweep(
                &mut k,
                &[Some(&circle)],
                Some(&path),
                None,
                None,
                Some(&translational),
            )
            .unwrap()
            .brep,
        ),
        (
            "path-scaled",
            create_curve_sweep(
                &mut k,
                &[Some(&circle)],
                Some(&reference),
                Some(&reference),
                Some(&scale),
                None,
            )
            .unwrap()
            .brep,
        ),
        (
            "path-closed",
            create_pipe_sweep(&mut k, RADIUS, Some(&closed), false).unwrap(),
        ),
    ];
    let ledger = path_ledger();
    let mut bare: BTreeSet<String> = BTreeSet::new();
    let mut definitions = 0;
    for (kind, b) in &cases {
        for s in walls(b) {
            if s.procedural_definition().is_some() {
                definitions += 1;
                continue;
            }
            if matches!(s.kind(), SurfaceKind::Plane(_)) {
                continue;
            }
            bare.insert((*kind).to_owned());
        }
    }
    assert!(definitions >= 5, "{definitions}");
    let unlisted: Vec<&String> = bare.iter().filter(|kd| !ledger.contains_key(*kd)).collect();
    assert!(
        unlisted.is_empty(),
        "bare wall faces of kinds the ledger does not list: {unlisted:?}"
    );
    // `path-spline` is the known-failing case's (`issue_2572_the_balance_wire_spline_pipe_is_in_the_ledger`).
    let fixed: Vec<&String> = ledger
        .keys()
        .filter(|kd| *kd != "path-spline" && !bare.contains(*kd))
        .collect();
    assert!(
        fixed.is_empty(),
        "listed kinds no case makes bare any more (fixed? delete their ledger files): {fixed:?}"
    );
}

/// The ledger's `path-spline` kind: the wire's pipe along its points fitted by a spline.
/// Known failing since #14640: that spline, fitted closed (C1 between the wire's two ends,
/// though they are apart), bends with radius about 0.041 near its start and 0.045 near its
/// end, so `create_pipe_sweep` refuses the wire's 0.75 pipe as folding through itself. The
/// input is the case's own and is not changed to pass (#15038): the case needs a path the
/// wire's pipe fits, and until it has one this test stays listed as failing.
#[test]
#[ignore = "known failing: the closed spline fit bends with radius 0.041 < 0.75 and the pipe is refused (#14640, #15038)"]
fn issue_2572_the_balance_wire_spline_pipe_is_in_the_ledger() {
    let mut k = Kernel::default();
    assert!(path_ledger().contains_key("path-spline"));
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&spline_path()), false).unwrap();
    let bare = walls(&b)
        .iter()
        .any(|s| s.procedural_definition().is_none() && !matches!(s.kind(), SurfaceKind::Plane(_)));
    assert!(
        bare,
        "the listed kind no longer appears (fixed? delete its ledger file)"
    );
}

/// The definition's partials at the ends of the net's parameter across the sweep are those
/// of its points, one-sided: the held parameter keeps the end segment's slope.
#[test]
fn the_definitions_partials_at_the_ends_are_its_slopes() {
    let mut k = Kernel::default();
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&arc_path()), false).unwrap();
    let wall = only_wall(&b);
    let sweep = wall.procedural_sweep().unwrap();
    let d = sweep.definition();
    let dom = sweep.net().natural_uv_domain();
    let (u, v) = (dom.u_interval(), dom.v_interval());
    let h = 1e-6;
    for end in 0..2 {
        for mid in [0.3, 0.7] {
            for along_u in [false, true] {
                // along the parameter whose end is tested; the other held at `mid`
                let (a, other) = if along_u { (u, v) } else { (v, u) };
                let at = if end == 0 { a.min() } else { a.max() };
                let inward = if end == 0 {
                    at + h * (a.max() - a.min())
                } else {
                    at - h * (a.max() - a.min())
                };
                let o = other.min() + (other.max() - other.min()) * mid;
                let make = |s: f64| {
                    if along_u {
                        Point2::new(s, o)
                    } else {
                        Point2::new(o, s)
                    }
                };
                let partials = d.evaluate_definition(make(at), 1, 1).unwrap();
                let analytic = if along_u {
                    partials[1][0]
                } else {
                    partials[0][1]
                };
                let p_end = d.evaluate_definition_point(make(at)).unwrap();
                let p_in = d.evaluate_definition_point(make(inward)).unwrap();
                let fd = (p_in - p_end) * (1.0 / (inward - at));
                assert!(
                    (analytic - fd).length() <= 1e-3 * (1.0 + analytic.length()),
                    "end {end}: {analytic:?} against {fd:?}"
                );
            }
        }
    }
}

/// At every interior joint of the wire the definition's partials along the path are the
/// right-sided slopes of its points: a span's start takes its own span's derivatives.
#[test]
fn the_definitions_partials_at_the_joints_are_right_sided() {
    let mut k = Kernel::default();
    let wire = wire_path(&k);
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&Curve::from(wire)), false).unwrap();
    let all = walls(&b);
    assert!(all.len() > 3);
    let mut joints = 0;
    for wall in &all {
        let s = wall.procedural_sweep().expect("a definition");
        let d = s.definition();
        let dom = s.net().natural_uv_domain();
        let (along_u, a) = if d.definition_map().2 {
            (true, dom.u_interval())
        } else {
            (false, dom.v_interval())
        };
        let other = if along_u {
            dom.v_interval()
        } else {
            dom.u_interval()
        };
        let t_start = d
            .path()
            .unwrap()
            .path_parameter(
                d.definition_parameters(Point2::new(dom.min().x, dom.min().y))
                    .y,
            )
            .0;
        if t_start <= d.path().unwrap().path().natural_interval().min() + 1e-12 {
            continue;
        }
        joints += 1;
        let h = 1e-7 * (a.max() - a.min());
        for mid in [0.2, 0.55, 0.8] {
            let o = other.min() + (other.max() - other.min()) * mid;
            let make = |x: f64| {
                if along_u {
                    Point2::new(x, o)
                } else {
                    Point2::new(o, x)
                }
            };
            let partials = d.evaluate_definition(make(a.min()), 1, 1).unwrap();
            let analytic = if along_u {
                partials[1][0]
            } else {
                partials[0][1]
            };
            let p0 = d.evaluate_definition_point(make(a.min())).unwrap();
            let p1 = d.evaluate_definition_point(make(a.min() + h)).unwrap();
            let fd = (p1 - p0) * (1.0 / h);
            assert!(
                (analytic - fd).length() <= 1e-4 * (1.0 + analytic.length()),
                "joint at {t_start}: {analytic:?} against {fd:?}"
            );
        }
    }
    assert!(joints >= 3, "{joints}");
}

/// The partials of a wall's definition along the path at both ends of the face's parameter
/// across the sweep, read through the face (`SweepRef`, asked for the definition itself),
/// are those of the face's own points taken inward, at every end that is an interior joint
/// of the wire. Returns how many ends were checked.
fn check_face_ends(k: &Kernel, wall: &Surface, what: &str) -> usize {
    let s = wall.procedural_sweep().expect("a definition");
    let d = s.definition();
    let dom = s.net().natural_uv_domain();
    let (along_u, a) = if d.definition_map().2 {
        (true, dom.u_interval())
    } else {
        (false, dom.v_interval())
    };
    let other = if along_u {
        dom.v_interval()
    } else {
        dom.u_interval()
    };
    let path_ivl = d.path().unwrap().path().natural_interval();
    let make = |x: f64, o: f64| {
        if along_u {
            Point2::new(x, o)
        } else {
            Point2::new(o, x)
        }
    };
    let h = 1e-7 * (a.max() - a.min());
    let mut checked = 0;
    for end in [a.min(), a.max()] {
        let o0 = other.min() + 0.4 * (other.max() - other.min());
        let wt = d.definition_parameters(make(end, o0));
        let t = d.path().unwrap().path_parameter(wt.y).0;
        if t <= path_ivl.min() + 1e-12 || t >= path_ivl.max() - 1e-12 {
            continue;
        }
        checked += 1;
        let inward = if end == a.min() { end + h } else { end - h };
        for mid in [0.2, 0.55, 0.8] {
            let o = other.min() + (other.max() - other.min()) * mid;
            let partials = s.evaluate(k, make(end, o), 1, 1).unwrap();
            let analytic = if along_u {
                partials[1][0]
            } else {
                partials[0][1]
            };
            let p_end = s.evaluate_point(k, make(end, o)).unwrap();
            let p_in = s.evaluate_point(k, make(inward, o)).unwrap();
            let fd = (p_in - p_end) * (1.0 / (inward - end));
            assert!(
                (analytic - fd).length() <= 1e-4 * (1.0 + analytic.length()),
                "{what}, end at t = {t}: {analytic:?} against {fd:?}"
            );
        }
    }
    checked
}

/// At the end of a face that ends at an interior joint of the wire the definition has the
/// partials of the face's own side, whether the face was reversed or swapped.
#[test]
fn the_definitions_partials_at_a_faces_ends_are_its_own_sides() {
    let mut k = Kernel::default();
    let wire = wire_path(&k);
    let b = create_pipe_sweep(&mut k, RADIUS, Some(&Curve::from(wire)), false).unwrap();
    let _exact = solidcore::surface::procedural_blend::NetScope::exact(&k);
    let mut checked = 0;
    for wall in &walls(&b) {
        checked += check_face_ends(&k, wall, "as built");
        for edit in 0..6 {
            let mut e = wall.copy(&k).unwrap();
            match edit {
                0 => e.reverse(&k, SurfParamType::U).unwrap(),
                1 => e.reverse(&k, SurfParamType::V).unwrap(),
                2 => e.swap_uv(&k).unwrap(),
                3 => {
                    e.swap_uv(&k).unwrap();
                    e.reverse(&k, SurfParamType::U).unwrap();
                }
                4 => {
                    e.swap_uv(&k).unwrap();
                    e.reverse(&k, SurfParamType::V).unwrap();
                }
                _ => {
                    e.reverse(&k, SurfParamType::U).unwrap();
                    e.reverse(&k, SurfParamType::V).unwrap();
                }
            }
            checked += check_face_ends(&k, &e, &format!("edit {edit}"));
        }
    }
    assert!(checked >= 20, "{checked}");
}
