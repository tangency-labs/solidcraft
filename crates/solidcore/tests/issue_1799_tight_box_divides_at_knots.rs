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
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::FRAC_1_SQRT_2;

use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::primitive::api_trim_ops::trim_surface_with_3d_curves;
use solidcore::query::face::p3::calculate_tight_bounding_box;
use solidcore::surface::surface::Surface;
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
