//! WR-651 (tangency-labs/smlib@326de73), the second half of #1799's trimmed-face tight box
//! (`Kernel::fixes.face_tight_box_searches_trim`, WR-588): the search divides an undecided
//! cell at the surface's knot inside it, not only at its middle.
//!
//! A rational patch is read monotone only within one knot span. Halved at middles alone, a
//! cell over a knot off the dyadic points of the face's domain never comes to lie within one
//! span, so along the trim it stays undecided down to the depth limit and counts with its
//! surface's top: the box overshoots. SolidCraft's case: an M10x1.5 thread cut from a shaft
//! of radius 5 and length 12 boxed to z 12.46875; its screw walls have a knot every eighth
//! of a turn.
//!
//! Here: the 270-degree cylinder of radius 1 about z (three rational quadratic quarter
//! spans, knots at u = 1/3 and 2/3), z = v over [0, 1], trimmed to z <= 0.3. Its box is
//! (-1, -1, 0) to (1, 1, 0.3).
//!
//! The first half, WR-588 itself, through an operation (#1954): a barrel with two bulges
//! (a cubic B-spline profile revolved a whole turn), its lower half cut away on one side by a
//! boolean difference, then the body's tight box. The face's surface peaks at the cut-away
//! bulge, outside the face; the face's own extreme is the upper bulge, inside it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::FRAC_1_SQRT_2;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::primitive::api_curves::{create_curve, create_line_segment};
use solidcore::primitive::api_primitives::create_box;
use solidcore::primitive::api_trim_ops::trim_surface_with_3d_curves;
use solidcore::query::api_queries::brep_bounding_box;
use solidcore::query::face::p3::calculate_tight_bounding_box;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::p1::faces;
use solidcore::Kernel;

/// The arc's knots: three quarter spans, double interior knots at 1/3 and 2/3.
fn arc_knots() -> Vec<f64> {
    let (a, b) = (1.0 / 3.0, 2.0 / 3.0);
    vec![0.0, 0.0, 0.0, a, a, b, b, 1.0, 1.0, 1.0]
}

/// The arc's control points `(x, y)` and weights, from angle 0 to 270 degrees.
fn arc_net() -> Vec<(f64, f64, f64)> {
    let s = FRAC_1_SQRT_2;
    vec![
        (1.0, 0.0, 1.0),
        (1.0, 1.0, s),
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, s),
        (-1.0, 0.0, 1.0),
        (-1.0, -1.0, s),
        (0.0, -1.0, 1.0),
    ]
}

/// The arc at height `z`, reversed when `backwards` (the knots are symmetric).
fn arc(z: f64, backwards: bool) -> Curve {
    let mut net = arc_net();
    if backwards {
        net.reverse();
    }
    let pw = net
        .into_iter()
        .map(|(x, y, w)| CPoint::new(x * w, y * w, z * w, w))
        .collect();
    let nurb = NurbCurve::new(pw, 2, arc_knots()).unwrap();
    Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
}

/// The cylinder: the arc along u, z = v linear over [0, 1].
fn cylinder() -> Surface {
    let net: Vec<Vec<CPoint>> = arc_net()
        .into_iter()
        .map(|(x, y, w)| {
            [0.0, 1.0]
                .iter()
                .map(|&z| CPoint::new(x * w, y * w, z * w, w))
                .collect()
        })
        .collect();
    let nurb = NurbSurface::new(net, 2, 1, arc_knots(), vec![0.0, 0.0, 1.0, 1.0]).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

fn line(from: Point3, to: Point3) -> Curve {
    let interval = Extent1d::from_min_max(0.0, 1.0);
    Curve::from(Line::create_line_segment(3, from, to, Some(&interval)).unwrap())
}

/// The tight box of the cylinder trimmed to `0 <= z <= top`, with the trim search on or off.
fn trimmed_cylinder_box(top: f64, searches_trim: bool) -> Extent3d {
    let mut k = Kernel::default();
    k.fixes.face_tight_box_searches_trim = searches_trim;
    let curves = vec![
        arc(0.0, false),
        line(Point3::new(0.0, -1.0, 0.0), Point3::new(0.0, -1.0, top)),
        arc(top, true),
        line(Point3::new(1.0, 0.0, top), Point3::new(1.0, 0.0, 0.0)),
    ];
    let b = trim_surface_with_3d_curves(&mut k, cylinder(), &[4], curves).unwrap();
    let fs = faces(&b, None).unwrap();
    assert_eq!(fs.len(), 1);
    calculate_tight_bounding_box(&k, &b, fs[0]).unwrap()
}

fn assert_close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() <= 1e-9, "{what}: {got}, want {want}");
}

/// With the search on, the cells over the knots divide there and are read monotone: the box
/// is the face's, exactly. Halved at middles only, the cells over u = 1/3 along the trim at
/// z = 0.3 stay undecided to the depth limit and the box reaches above 0.3.
#[test]
fn trimmed_rational_cylinder_box_is_exact_with_the_search() {
    let bx = trimmed_cylinder_box(0.3, true);
    assert_close("z max", bx.max().z, 0.3);
    assert_close("z min", bx.min().z, 0.0);
    assert_close("x max", bx.max().x, 1.0);
    assert_close("x min", bx.min().x, -1.0);
    assert_close("y max", bx.max().y, 1.0);
    assert_close("y min", bx.min().y, -1.0);
}

/// Flag off (upstream): the surface's global solves classify outside the face, so the box
/// is the vertices' and edges', which here is the face's too.
#[test]
fn trimmed_rational_cylinder_box_without_the_search_is_the_edges() {
    let bx = trimmed_cylinder_box(0.3, false);
    assert_close("z max", bx.max().z, 0.3);
    assert_close("z min", bx.min().z, 0.0);
}

/// The barrel's outer profile in the plane y = 0: a cubic B-spline from (5, 0, 0) to
/// (5, 0, 10) with a tall bulge near z = 2 and a lower one near z = 8.
fn barrel_profile() -> Curve {
    let p = |x: f64, z: f64| Point3::new(x, 0.0, z);
    let points = [
        p(5.0, 0.0),
        p(5.0, 1.0),
        p(12.0, 2.0),
        p(5.0, 3.0),
        p(5.0, 4.5),
        p(5.0, 5.5),
        p(8.5, 8.0),
        p(5.0, 9.0),
        p(5.0, 10.0),
    ];
    Curve::from(create_curve(&points).unwrap())
}

/// The profile closed by lines round a bore of radius 1, revolved a whole turn about z.
fn barrel(k: &mut Kernel) -> solidcore::Brep {
    let p = |x: f64, z: f64| Point3::new(x, 0.0, z);
    let curves = [
        barrel_profile(),
        Curve::from(create_line_segment(p(5.0, 10.0), p(1.0, 10.0)).unwrap()),
        Curve::from(create_line_segment(p(1.0, 10.0), p(1.0, 0.0)).unwrap()),
        Curve::from(create_line_segment(p(1.0, 0.0), p(5.0, 0.0)).unwrap()),
    ];
    let refs: Vec<_> = curves.iter().map(Some).collect();
    create_rotational_sweep(k, &refs, Point3::new(0.0, 0.0, 0.0), Vec3::Z, 360.0, true).unwrap()
}

/// The profile's point at parameter `t`.
fn profile_at(k: &Kernel, c: &Curve, t: f64) -> Point3 {
    c.evaluate_point(k, t).unwrap()
}

/// The profile's greatest radius (x) where z lies in `lo..hi`, from the curve itself: the
/// best of 4096 samples, then a golden-section search round it.
fn profile_max_x(lo: f64, hi: f64) -> f64 {
    let k = Kernel::default();
    let c = barrel_profile();
    let ivl = c.natural_interval();
    let n = 4096;
    let step = (ivl.max() - ivl.min()) / f64::from(n);
    let x_at = |t: f64| {
        let q = profile_at(&k, &c, t);
        if q.z > lo && q.z < hi {
            q.x
        } else {
            f64::NEG_INFINITY
        }
    };
    let best = (0..=n)
        .map(|i| ivl.min() + step * f64::from(i))
        .max_by(|a, b| x_at(*a).total_cmp(&x_at(*b)))
        .unwrap();
    let (mut a, mut b) = (best - step, best + step);
    let g = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..200 {
        let (c1, c2) = (b - g * (b - a), a + g * (b - a));
        if x_at(c1) < x_at(c2) {
            a = c1;
        } else {
            b = c2;
        }
    }
    x_at((a + b) / 2.0)
}

/// The profile's radius (x) at height `z` (z rises along the profile), by bisection.
fn profile_x_at_z(z: f64) -> f64 {
    let k = Kernel::default();
    let c = barrel_profile();
    let ivl = c.natural_interval();
    let (mut a, mut b) = (ivl.min(), ivl.max());
    for _ in 0..200 {
        let m = (a + b) / 2.0;
        if profile_at(&k, &c, m).z < z {
            a = m;
        } else {
            b = m;
        }
    }
    profile_at(&k, &c, (a + b) / 2.0).x
}

/// WR-588 at operation level (#1954): the barrel's tight box is its surface's either way.
/// Cut by a boolean difference of the box over y >= 0, z <= 5, the tall bulge stays on the
/// face only on the side y < 0. The face's +y extreme is the lower bulge's crest, inside the
/// face. Off (pinned upstream), the global solve finds the cut-away bulge, classifies it
/// outside the face and drops it: the body's +y side is the cut edge round the waist at
/// z = 5. On (the fork), the trim search finds the crest. The other five sides agree.
#[test]
fn wr_588_barrel_cut_below_its_waist_is_boxed_to_its_upper_bulge() {
    let tall = profile_max_x(0.0, 5.0);
    let low = profile_max_x(5.0, 10.0);
    let waist = profile_x_at_z(5.0);
    assert!(
        waist < low - 1.0 && low < tall - 1.0,
        "{waist} {low} {tall}"
    );
    for searches_trim in [false, true] {
        let mut k = Kernel::default();
        k.fixes.face_tight_box_searches_trim = searches_trim;
        let whole = barrel(&mut k);
        // control: uncut, the surface's global extremes are the face's
        let bx = brep_bounding_box(&k, &whole, true).unwrap();
        for (what, got, want) in [
            ("x max", bx.max().x, tall),
            ("x min", bx.min().x, -tall),
            ("y max", bx.max().y, tall),
            ("y min", bx.min().y, -tall),
            ("z max", bx.max().z, 10.0),
            ("z min", bx.min().z, 0.0),
        ] {
            assert!(
                (got - want).abs() <= 1e-9,
                "{searches_trim}: whole {what}: {got}, want {want}"
            );
        }

        let cut = create_box(&mut k, Point3::new(-20.0, 0.0, -1.0), 40.0, 20.0, 6.0).unwrap();
        let body = boolean_difference(&mut k, whole, cut).unwrap();
        assert_eq!(faces(&body, None).unwrap().len(), 7, "{searches_trim}");
        let bx = brep_bounding_box(&k, &body, true).unwrap();
        let want_y = if searches_trim { low } else { waist };
        assert!(
            (bx.max().y - want_y).abs() <= 1e-9,
            "{searches_trim}: y max {}, want {want_y}",
            bx.max().y
        );
        for (what, got, want) in [
            ("x max", bx.max().x, tall),
            ("x min", bx.min().x, -tall),
            ("y min", bx.min().y, -tall),
            ("z max", bx.max().z, 10.0),
            ("z min", bx.min().z, 0.0),
        ] {
            assert!(
                (got - want).abs() <= 1e-9,
                "{searches_trim}: cut {what}: {got}, want {want}"
            );
        }
    }
}
