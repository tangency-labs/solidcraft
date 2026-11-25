//! Tests of the planar prism primitives: closed forms, the bounds, moved inputs refused, and the
//! definition (a planar piece) round-tripped through the kernel's curve.

#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use super::extrude::{extrude_region, symmetric_offset_intervals, symmetric_offset_probe_points};
use super::measure::{equivalent, section};
use super::placement::LoopPlacement;
use super::profile::rational_curve;
use super::profile::{
    check_regions, curve_on_frame, largest_arc_sweep, smallest_arc_sweep, tapered_flange_ends,
    Loop2, ProfileRefusal, Region2, Seg2,
};
use super::read::{
    arc_before_line, circular_boundary, planar_curves, planar_pieces, PlanarPiece, Run,
};
use super::sheet::{region_loops, sheet_regions};
use crate::boolean::api_brep::{boolean_2d, Boolean2dOutcome};
use crate::curve::curve::{Curve, CurveKind};
use crate::eval::Frame;
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::math::{Point2, Vec2};
use crate::primitive::api_trim_ops::create_planar_faces;
use crate::primitive::primitive_creation::Boolean2d;
use crate::topology::Brep;

/// The default kernel's zone tolerance, which the profile checks here build at.
const ZONE: f64 = 1e-5;

fn xy() -> Frame {
    Frame {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    }
}

fn p(x: f64, y: f64) -> Point2 {
    Vec2::new(x, y)
}

#[test]
fn chord_coordinates_preserve_aspect_ratio_and_refuse_unrepresentable_inputs() {
    use super::transform::chord_coordinates;

    // c = 1.5 - 0.25 = 1.25; 0.625/c = 0.5, 0.15625/c = 0.125.
    assert_eq!(
        chord_coordinates(p(0.875, 0.15625), 0.25, 1.5).unwrap(),
        p(0.5, 0.125)
    );
    assert_eq!(
        chord_coordinates(p(0.25, 0.0), 0.25, 1.5).unwrap(),
        Vec2::ZERO
    );
    assert_eq!(
        chord_coordinates(p(1.5, 0.0), 0.25, 1.5).unwrap(),
        p(1.0, 0.0)
    );
    assert_eq!(
        chord_coordinates(p(0.5, -0.125), 0.0, 1.0).unwrap(),
        p(0.5, -0.125)
    );
    for (point, leading, trailing) in [
        (p(0.0, 0.0), 0.0, 0.0),
        (p(0.0, 0.0), 1.0, 0.0),
        (p(f64::NAN, 0.0), 0.0, 1.0),
        (p(0.0, f64::INFINITY), 0.0, 1.0),
        (p(0.0, 0.0), f64::NEG_INFINITY, 1.0),
        (p(0.0, 0.0), 0.0, f64::INFINITY),
        (p(0.0, 0.0), -f64::MAX, f64::MAX),
        (p(f64::MAX, 0.0), -f64::MAX, 0.0),
        (p(0.0, 1.0), 0.0, f64::from_bits(1)),
    ] {
        assert!(
            chord_coordinates(point, leading, trailing).is_err(),
            "{point:?}, {leading}, {trailing}"
        );
    }
}

fn polygon(pts: &[Point2]) -> Loop2 {
    Loop2 {
        segs: pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| Seg2::Line { a: *a, b: *b })
            .collect(),
    }
}

fn rectangle(a: Point2, b: Point2) -> Region2 {
    Region2 {
        outer: polygon(&[a, p(b.x, a.y), b, p(a.x, b.y)]),
        holes: vec![],
    }
}

fn circle(c: Point2, r: f64) -> Loop2 {
    Loop2 {
        segs: vec![Seg2::Arc {
            center: c,
            radius: r,
            start: 0.0,
            sweep: TAU,
        }],
    }
}

/// An ellipse about `c` with semi-axes `a` (along x) and `b`, as four exact conics.
fn ellipse(c: Point2, a: f64, b: f64) -> Loop2 {
    let corners = [c + p(a, 0.0), c + p(0.0, b), c - p(a, 0.0), c - p(0.0, b)];
    Loop2 {
        segs: corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .map(|(s, e)| Seg2::Conic {
                a: *s,
                apex: *s + *e - c,
                b: *e,
                w: FRAC_1_SQRT_2,
            })
            .collect(),
    }
}

/// Encloses `exact` within the area's bound, the bound no wider than `width`.
fn encloses(lp: &Loop2, exact: f64, width: f64) {
    let a = lp.signed_area().unwrap();
    assert!(
        (a.value - exact).abs() <= a.bound,
        "{} vs {exact} (bound {})",
        a.value,
        a.bound
    );
    assert!(a.bound <= width, "bound {}", a.bound);
}

#[test]
fn areas_in_closed_form_are_enclosed_by_their_bounds() {
    encloses(
        &polygon(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]),
        100.0,
        1e-12,
    );
    encloses(&circle(p(3.0, -2.0), 6.0), 36.0 * PI, 1e-11);
    encloses(&circle(p(3.0, -2.0), 6.0).reversed(), -36.0 * PI, 1e-11);
    encloses(&ellipse(p(40.0, 30.0), 15.0, 8.0), PI * 120.0, 1e-9);
    // a parabolic arc (a conic of weight 1) and its chord: two thirds of the control triangle
    let parabola = Loop2 {
        segs: vec![
            Seg2::Conic {
                a: p(0.0, 0.0),
                apex: p(1.0, 2.0),
                b: p(2.0, 0.0),
                w: 1.0,
            },
            Seg2::Line {
                a: p(2.0, 0.0),
                b: p(0.0, 0.0),
            },
        ],
    };
    encloses(&parabola.reversed(), 4.0 / 3.0, 1e-13);
    // the same parabola as a quadratic Bézier, and degree-elevated to a cubic
    let mut q = [p(0.0, 0.0); 10];
    q[1] = p(1.0, 2.0);
    q[2] = p(2.0, 0.0);
    let quad = Seg2::Bezier { n: 2, p: q };
    let cubic = Seg2::Cubic {
        p0: p(0.0, 0.0),
        p1: p(2.0 / 3.0, 4.0 / 3.0),
        p2: p(4.0 / 3.0, 4.0 / 3.0),
        p3: p(2.0, 0.0),
    };
    for s in [quad, cubic] {
        encloses(
            &Loop2 {
                segs: vec![
                    Seg2::Line {
                        a: p(0.0, 0.0),
                        b: p(2.0, 0.0),
                    },
                    s.reversed(),
                ],
            },
            4.0 / 3.0,
            1e-13,
        );
    }
    // a circular arc as a conic (w the cosine of its half angle): its sector less the triangle,
    // past the series' range (75 degrees) and inside it (30 degrees)
    for half in [75.0_f64.to_radians(), 30.0_f64.to_radians()] {
        let (s, c) = half.sin_cos();
        let arc = Seg2::Conic {
            a: p(c, -s),
            apex: p(1.0 / c, 0.0),
            b: p(c, s),
            w: c,
        };
        encloses(
            &Loop2 {
                segs: vec![
                    arc,
                    Seg2::Line {
                        a: p(c, s),
                        b: p(c, -s),
                    },
                ],
            },
            half - s * c,
            1e-13,
        );
    }
    // x^2 - y^2 = 1 from (cosh 1, -sinh 1) to (cosh 1, sinh 1), w = cosh 1: the segment between
    // the hyperbola and its chord is sinh 1 cosh 1 - 1
    let (sh, ch) = (1.0_f64.sinh(), 1.0_f64.cosh());
    let hyper = Seg2::Conic {
        a: p(ch, -sh),
        apex: p(1.0 / ch, 0.0),
        b: p(ch, sh),
        w: ch,
    };
    encloses(
        &Loop2 {
            segs: vec![
                hyper,
                Seg2::Line {
                    a: p(ch, sh),
                    b: p(ch, -sh),
                },
            ],
        }
        .reversed(),
        sh * ch - 1.0,
        1e-12,
    );
}

#[test]
fn holds_decides_exactly_or_refuses_on_the_boundary() {
    let c = circle(p(0.0, 0.0), 10.0);
    // inside the arc where a 1e-3 chord would have cut it off
    assert_eq!(c.holds(p(9.99999, 0.001)), Some(true));
    assert_eq!(c.holds(p(10.00001, 0.0)), Some(false));
    assert_eq!(c.holds(p(10.0, 0.0)), None);
    let e = ellipse(p(40.0, 30.0), 15.0, 8.0);
    assert_eq!(e.holds(p(40.0, 37.999)), Some(true));
    assert_eq!(e.holds(p(54.0, 33.0)), Some(false));
    let sq = polygon(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]);
    assert_eq!(sq.holds(p(5.0, 0.0)), None);
    assert_eq!(sq.holds(p(5.0, 5.0)), Some(true));
    assert_eq!(sq.holds(p(-1.0, 5.0)), Some(false));
}

#[test]
fn holds_decides_a_point_off_the_loop_on_an_edge_line_by_the_half_open_rule() {
    let sq = polygon(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]);
    // on the line of a horizontal edge, either side of the square
    assert_eq!(sq.holds(p(-1.0, 0.0)), Some(false));
    assert_eq!(sq.holds(p(11.0, 0.0)), Some(false));
    assert_eq!(sq.holds(p(-1.0, 10.0)), Some(false));
    // on the line of a vertical edge, the ray through both of its corners
    assert_eq!(sq.holds(p(5.0, -1.0)), Some(false));
    assert_eq!(sq.holds(p(-1e-3, 10.0)), Some(false));
    // level with a vertex the ray passes through, inside and outside
    let diamond = polygon(&[p(0.0, -5.0), p(5.0, 0.0), p(0.0, 5.0), p(-5.0, 0.0)]);
    assert_eq!(diamond.holds(p(0.0, 0.0)), Some(true));
    assert_eq!(diamond.holds(p(-6.0, 0.0)), Some(false));
    // on the loop: refused
    assert_eq!(sq.holds(p(0.0, 0.0)), None);
    assert_eq!(sq.holds(p(10.0, 5.0)), None);
    // a hole's level edges seen from inside the plate
    let hole = polygon(&[p(2.0, 2.0), p(2.0, 8.0), p(8.0, 8.0), p(8.0, 2.0)]);
    assert_eq!(hole.holds(p(1.0, 2.0)), Some(false));
    assert_eq!(hole.holds(p(5.0, 2.0 + 1e-9)), Some(true));
}

/// A clockwise circle about `c` as four quarter arcs (a hole's loop).
fn clockwise_quarters(c: Point2, r: f64) -> Loop2 {
    Loop2 {
        segs: (0..4)
            .map(|i| Seg2::Arc {
                center: c,
                radius: r,
                start: -FRAC_PI_2 * f64::from(i),
                sweep: -FRAC_PI_2,
            })
            .collect(),
    }
}

/// The distance from a point to a line or an arc is closed form: the foot clamped to the line's
/// ends, the radial distance on an arc where the point is within its angles (and from its centre
/// any point of the circle), else the nearer end; a free-form segment and a point that is no number
/// have none.
#[test]
fn distance_to_a_line_or_an_arc_is_exact_and_a_curve_has_none() {
    let line = Seg2::Line {
        a: p(0.0, 0.0),
        b: p(10.0, 0.0),
    };
    assert_eq!(line.distance_to(p(5.0, 3.0)), Some(3.0));
    assert_eq!(line.distance_to(p(5.0, 0.0)), Some(0.0));
    // Past an end: the distance to the end, 3-4-5.
    assert_eq!(line.distance_to(p(-3.0, 4.0)), Some(5.0));
    assert_eq!(line.distance_to(p(13.0, -4.0)), Some(5.0));
    // A quarter arc of radius 5 about the origin, from the +x axis to the +y axis.
    let arc = Seg2::Arc {
        center: p(0.0, 0.0),
        radius: 5.0,
        start: 0.0,
        sweep: FRAC_PI_2,
    };
    assert_eq!(
        arc.distance_to(p(10.0, 0.0)),
        Some(5.0),
        "within its angles, radially"
    );
    assert_eq!(arc.distance_to(p(1.0, 0.0)), Some(4.0), "inside the circle");
    assert_eq!(
        arc.distance_to(p(0.0, 0.0)),
        Some(5.0),
        "from its centre, any point of it"
    );
    // Outside its angles: the nearer end, (5, 0): sqrt(5^2 + 5^2) from (0, -5).
    let d = arc.distance_to(p(0.0, -5.0)).unwrap();
    assert!((d - 50.0_f64.sqrt()).abs() < 1e-12, "{d}");
    // A clockwise arc measures the same way.
    let back = Seg2::Arc {
        center: p(0.0, 0.0),
        radius: 5.0,
        start: FRAC_PI_2,
        sweep: -FRAC_PI_2,
    };
    assert_eq!(back.distance_to(p(10.0, 0.0)), Some(5.0));
    // Free-form: none.
    let curve = Seg2::Cubic {
        p0: p(0.0, 0.0),
        p1: p(1.0, 2.0),
        p2: p(3.0, 2.0),
        p3: p(4.0, 0.0),
    };
    assert_eq!(curve.distance_to(p(2.0, 5.0)), None);
    // Not a number: none, never a NaN.
    assert_eq!(line.distance_to(p(f64::NAN, 1.0)), None);
    assert_eq!(arc.distance_to(p(f64::INFINITY, 0.0)), None);
}

#[test]
fn placement_decides_inside_outside_along_and_crossing_with_arcs_exact() {
    let tol = 1e-7;
    let hole = clockwise_quarters(p(0.0, 0.0), 1.0);
    // Starting at (0.5, -0.5): on the chord of the quarter arc from (1, 0) to (0, -1), inside the
    // arc. A winding count that takes the chord for the arc there finds it outside.
    let sq = polygon(&[p(0.5, -0.5), p(0.6, -0.5), p(0.6, -0.4), p(0.5, -0.4)]);
    assert_eq!(sq.placement(&hole, tol), Some(LoopPlacement::Inside));
    assert_eq!(
        sq.reversed().placement(&hole, tol),
        Some(LoopPlacement::Inside)
    );
    let beyond = polygon(&[p(0.8, -0.8), p(0.9, -0.8), p(0.9, -0.7), p(0.8, -0.7)]);
    assert_eq!(beyond.placement(&hole, tol), Some(LoopPlacement::Outside));
    // The same circle as one whole arc run the other way: along it, both ways round; within the
    // distance still along it, past it clear outside.
    let whole = circle(p(0.0, 0.0), 1.0);
    assert_eq!(whole.placement(&hole, tol), Some(LoopPlacement::Coincident));
    assert_eq!(hole.placement(&whole, tol), Some(LoopPlacement::Coincident));
    let near = circle(p(0.0, 0.0), 1.0 + tol / 2.0);
    assert_eq!(near.placement(&hole, tol), Some(LoopPlacement::Coincident));
    let clear = circle(p(0.0, 0.0), 1.0 + 2.0 * tol);
    assert_eq!(clear.placement(&hole, tol), Some(LoopPlacement::Outside));
    // Crossing the circle at two points, and touching it from inside at one.
    let across = circle(p(0.8, 0.0), 0.5);
    assert_eq!(across.placement(&hole, tol), Some(LoopPlacement::Crossing));
    let touching = circle(p(0.5, 0.0), 0.5);
    assert_eq!(
        touching.placement(&hole, tol),
        Some(LoopPlacement::Crossing)
    );
    // Lines: a square along the same square split at another vertex, one along part of it only,
    // and one clear inside it.
    let square = polygon(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]);
    let split = polygon(&[
        p(0.0, 0.0),
        p(4.0, 0.0),
        p(10.0, 0.0),
        p(10.0, 10.0),
        p(0.0, 10.0),
    ]);
    assert_eq!(
        square.placement(&split, tol),
        Some(LoopPlacement::Coincident)
    );
    assert_eq!(
        split.reversed().placement(&square, tol),
        Some(LoopPlacement::Coincident)
    );
    let half = polygon(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0), p(0.0, 5.0)]);
    assert_eq!(half.placement(&square, tol), Some(LoopPlacement::Crossing));
    let small = polygon(&[p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0), p(2.0, 8.0)]);
    assert_eq!(small.placement(&square, tol), Some(LoopPlacement::Inside));
    assert_eq!(square.placement(&small, tol), Some(LoopPlacement::Outside));
    // Refused: a free-form loop, no positive distance.
    assert_eq!(ellipse(p(0.0, 0.0), 3.0, 2.0).placement(&hole, tol), None);
    assert_eq!(sq.placement(&hole, 0.0), None);
    assert_eq!(sq.placement(&hole, f64::NAN), None);
}

#[test]
fn hostile_profiles_are_refused_by_kind() {
    let plate = rectangle(p(0.0, 0.0), p(10.0, 10.0));
    let mut bad = plate.clone();
    bad.outer.segs[0] = Seg2::Line {
        a: p(f64::NAN, 0.0),
        b: p(1.0, 0.0),
    };
    assert_eq!(
        check_regions([&bad].into_iter(), 1e-7, ZONE),
        Err(ProfileRefusal::LineOrArc)
    );
    let mut conic = Region2 {
        outer: ellipse(p(40.0, 30.0), 15.0, 8.0),
        holes: vec![],
    };
    if let Seg2::Conic { apex, .. } = &mut conic.outer.segs[1] {
        apex.x = f64::NAN;
    }
    assert_eq!(
        check_regions([&conic].into_iter(), 1e-7, ZONE),
        Err(ProfileRefusal::FreeForm)
    );
    let open = Region2 {
        outer: Loop2 {
            segs: plate.outer.segs[..3].to_vec(),
        },
        holes: vec![],
    };
    assert_eq!(
        check_regions([&open].into_iter(), 1e-7, ZONE),
        Err(ProfileRefusal::Budget)
    );
    assert_eq!(check_regions([&plate].into_iter(), 1e-7, ZONE), Ok(()));
}

fn run<'a>(k: &Kernel, c: &'a Curve, interval: Extent1d, forward: bool) -> Run<'a> {
    let ends = [
        c.evaluate_point(k, interval.min()).unwrap(),
        c.evaluate_point(k, interval.max()).unwrap(),
    ];
    Run {
        curve: c,
        interval,
        forward,
        ends: if forward { ends } else { [ends[1], ends[0]] },
    }
}

fn read(
    k: &Kernel,
    c: &Curve,
    interval: Extent1d,
    forward: bool,
    frame: &Frame,
    tol: f64,
) -> Option<Vec<Seg2>> {
    planar_pieces(
        k,
        &run(k, c, interval, forward),
        frame,
        tol,
        &mut 2_000_000,
        false,
    )
}

#[allow(clippy::needless_pass_by_value)] // the fixtures build their vectors in the call
fn spline(_k: &Kernel, knots: Vec<f64>, homogeneous: Vec<[f64; 4]>) -> (Curve, Extent1d) {
    let c = rational_curve(&knots, &homogeneous).unwrap();
    let iv = c.natural_interval();
    (c, iv)
}

/// A segment made the kernel's curve on a frame reads back as itself: the definition a record
/// stores is the one the kernel keeps (a planar piece's round trip).
#[test]
fn a_piece_made_a_curve_reads_back_as_itself() {
    let k = Kernel::default();
    let tilted = Frame {
        origin: [5.0, -3.0, 2.0],
        x_axis: [0.6, 0.8, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    };
    let pieces = [
        Seg2::Line {
            a: p(1.0, 2.0),
            b: p(-4.0, 7.5),
        },
        Seg2::Arc {
            center: p(1.0, 1.0),
            radius: 3.0,
            start: 0.3,
            sweep: 2.0,
        },
        Seg2::Arc {
            center: p(1.0, 1.0),
            radius: 3.0,
            start: 0.3,
            sweep: -2.0,
        },
        Seg2::Conic {
            a: p(0.0, 0.0),
            apex: p(1.0, 2.0),
            b: p(3.0, 0.5),
            w: 0.4,
        },
        Seg2::Cubic {
            p0: p(0.0, 0.0),
            p1: p(1.0, 3.0),
            p2: p(2.0, -3.0),
            p3: p(3.0, 0.0),
        },
    ];
    for frame in [xy(), tilted] {
        for s in pieces {
            let c = curve_on_frame(&k, &frame, 1.5, &s).unwrap();
            let iv = match c.kind() {
                CurveKind::Line(_) => Extent1d::from_raw(0.0, 1.0),
                _ => c.natural_interval(),
            };
            let back = read(&k, &c, iv, true, &frame, 1e-7).unwrap_or_else(|| panic!("{s:?}"));
            assert_eq!(back.len(), 1, "{s:?}: {back:?}");
            let b = back[0];
            assert!(
                b.start().distance(s.start()) < 1e-9 && b.end().distance(s.end()) < 1e-9,
                "{s:?} vs {b:?}"
            );
            assert_eq!(
                std::mem::discriminant(&b),
                std::mem::discriminant(&s),
                "{s:?} vs {b:?}"
            );
            if let (
                Seg2::Arc {
                    center,
                    radius,
                    sweep,
                    ..
                },
                Seg2::Arc {
                    center: c0,
                    radius: r0,
                    sweep: s0,
                    ..
                },
            ) = (b, s)
            {
                assert!(
                    center.distance(c0) < 1e-9
                        && (radius - r0).abs() < 1e-9
                        && (sweep - s0).abs() < 1e-9,
                    "{b:?}"
                );
            }
            if let (
                Seg2::Conic { apex, w, .. },
                Seg2::Conic {
                    apex: a0, w: w0, ..
                },
            ) = (b, s)
            {
                assert!(apex.distance(a0) < 1e-9 && (w - w0).abs() < 1e-12, "{b:?}");
            }
        }
    }
}

/// The absolute tolerance of the arc closed-form checks below. Chosen, not derived: the
/// coordinates they compare stay within |x| <= 5, where an ulp is at most 8.9e-16, so this is
/// about 1000 ulps.
const ARC_CLOSED_FORM_TOL: f64 = 1e-12;

/// `center + radius (cos t, sin t)` on the plain frame.
fn on_circle(center: Point2, radius: f64, t: f64) -> [f64; 3] {
    [
        center.x + radius * t.cos(),
        center.y + radius * t.sin(),
        0.0,
    ]
}

/// The largest coordinate difference between `q` and `want`.
fn off(q: crate::geom::vector3d::Point3, want: [f64; 3]) -> f64 {
    [q.x, q.y, q.z]
        .into_iter()
        .zip(want)
        .map(|(g, w)| (g - w).abs())
        .fold(0.0, f64::max)
}

/// The arc `curve_on_frame` builds on the plain frame against its closed form: it starts at
/// `center + radius (cos start, sin start)` and ends at the same with `start + sweep`, every point
/// between lies on the circle, and its angle from the start, turned the way `sweep` runs, rises
/// strictly from 0 to `|sweep|` (the curve's own parameter need not be linear in the angle).
fn assert_arc_closed_form(k: &Kernel, center: Point2, radius: f64, start: f64, sweep: f64) {
    let s = Seg2::Arc {
        center,
        radius,
        start,
        sweep,
    };
    let c = curve_on_frame(k, &xy(), 0.0, &s).unwrap();
    let iv = c.natural_interval();
    let a = c.evaluate_point(k, iv.min()).unwrap();
    let b = c.evaluate_point(k, iv.max()).unwrap();
    for (q, t) in [(a, start), (b, start + sweep)] {
        let want = on_circle(center, radius, t);
        assert!(
            off(q, want) < ARC_CLOSED_FORM_TOL,
            "{s:?}: {q:?} vs {want:?}"
        );
    }
    let mut last = 0.0;
    for i in 1..64 {
        let f = f64::from(i) / 64.0;
        let q = c
            .evaluate_point(k, iv.min() + f * (iv.max() - iv.min()))
            .unwrap();
        let (dx, dy) = (q.x - center.x, q.y - center.y);
        assert!(
            (dx.hypot(dy) - radius).abs() < ARC_CLOSED_FORM_TOL && q.z.abs() < ARC_CLOSED_FORM_TOL,
            "{s:?} at f = {f}: {q:?} off the circle"
        );
        let turned = (sweep.signum() * (dy.atan2(dx) - start)).rem_euclid(TAU);
        assert!(
            turned > last && turned < sweep.abs(),
            "{s:?} at f = {f}: angle {turned} after {last}"
        );
        last = turned;
    }
}

/// A single arc that is a whole turn, starting away from the frame's `x`: its own circle, angle
/// 0 at the start, so it closes at the start and runs through the closed form all the way round.
#[test]
fn a_full_circle_arc_at_a_nonzero_start_is_the_closed_form() {
    let k = Kernel::default();
    assert_arc_closed_form(&k, p(1.0, -2.0), 3.0, 1.0, TAU);
    assert_arc_closed_form(&k, p(1.0, -2.0), 3.0, 1.0, -TAU);
}

/// A counter-clockwise arc whose start is past a whole turn is the same arc as at `start - 2π`:
/// its trim never crosses a seam, whatever the start.
#[test]
fn a_ccw_arc_starting_past_a_whole_turn_is_the_closed_form() {
    let k = Kernel::default();
    assert_arc_closed_form(&k, p(0.5, 0.25), 2.0, TAU + 1.25, 1.5);
    assert_arc_closed_form(&k, p(0.5, 0.25), 2.0, 3.0 * TAU - 0.5, 2.0);
}

/// The sweep limits come from the radius and the kernel's circle builder (`tol` the default zone
/// tolerance; `floor` the builder's 1e-6 degree smallest angle and `win` its 0.01 degree
/// full-turn window, each plus the margin): at r = 1 and r = 1e8, on both sides of `tol / r`,
/// `floor` and `win`. Refused: at most [`smallest_arc_sweep`], past [`largest_arc_sweep`], or
/// inside the window short of a turn with the end farther than `tol`. Built: the arc's ends in
/// closed form (to the rounding of coordinates of size r), or the closed circle with its end
/// within `tol` of `start + sweep`. `check_regions` accepts exactly the arcs `curve_on_frame`
/// builds.
#[test]
fn an_arc_builds_exactly_within_the_limits_its_radius_sets() {
    let k = Kernel::default();
    let tol = k.tolerances.zone_tol_3d;
    let floor = (1e-6_f64 + 1e-11).to_radians();
    let win = (0.01_f64 + 1e-11).to_radians();
    for r in [1.0, 1e8] {
        let arc = |sweep: f64| Seg2::Arc {
            center: p(0.0, 0.0),
            radius: r,
            start: 0.5,
            sweep,
        };
        let exact = r * 1e-14 + ARC_CLOSED_FORM_TOL;
        assert_eq!(largest_arc_sweep(r, tol), TAU + tol / r);
        assert_eq!(smallest_arc_sweep(r, tol), (tol / r).max(floor));
        // tol / r is inside the window at both radii (1e-5 and 1e-13 against 1.7e-4)
        let refused = [
            0.0,
            0.5 * tol / r,
            0.99 * floor,
            smallest_arc_sweep(r, tol),
            TAU + 2.0 * tol / r,
            TAU - 2.0 * tol / r,
            TAU - 0.99 * win,
            7.0,
        ];
        // the closed circle, its end within tol of the arc's
        let closed = [TAU, TAU + 0.5 * tol / r, TAU - 0.5 * tol / r];
        // an arc, its ends the closed form
        let open = [1.01 * smallest_arc_sweep(r, tol), TAU - 1.01 * win, 2.0];
        let ends = |sweep: f64| {
            let c = curve_on_frame(&k, &xy(), 0.0, &arc(sweep)).unwrap();
            let iv = c.natural_interval();
            let a = c.evaluate_point(&k, iv.min()).unwrap();
            let b = c.evaluate_point(&k, iv.max()).unwrap();
            (a, b)
        };
        let start = on_circle(p(0.0, 0.0), r, 0.5);
        for &s in &refused {
            for sweep in [s, -s] {
                let c = curve_on_frame(&k, &xy(), 0.0, &arc(sweep));
                assert!(
                    matches!(
                        c,
                        Err(crate::error::SolidError::InvalidInput {
                            operation: "prism::profile::curve_on_frame",
                            ..
                        })
                    ),
                    "r {r}, sweep {sweep}: {c:?}"
                );
            }
        }
        for &s in &open {
            for sweep in [s, -s] {
                let (a, b) = ends(sweep);
                let end = on_circle(p(0.0, 0.0), r, 0.5 + sweep);
                assert!(
                    off(a, start) <= exact && off(b, end) <= exact,
                    "r {r}, sweep {sweep}: {a:?} {b:?} vs {start:?} {end:?}"
                );
            }
        }
        for &s in &closed {
            for sweep in [s, -s] {
                let (a, b) = ends(sweep);
                let end = on_circle(p(0.0, 0.0), r, 0.5 + sweep);
                assert!(
                    off(a, start) <= exact && off(b, start) <= exact,
                    "r {r}, sweep {sweep}: {a:?} {b:?} not closed at {start:?}"
                );
                assert!(off(b, end) <= tol, "r {r}, sweep {sweep}: {b:?} vs {end:?}");
            }
        }
        // the profile check accepts exactly what builds (a loop of the arc, closed by a line
        // where its ends are apart; checked at a closure tolerance any of them meets)
        for &s in refused.iter().chain(&open).chain(&closed) {
            for sweep in [s, -s] {
                let seg = arc(sweep);
                let mut segs = vec![seg];
                if seg.start().distance(seg.end()) > 1e-6 {
                    segs.push(Seg2::Line {
                        a: seg.end(),
                        b: seg.start(),
                    });
                }
                let region = Region2 {
                    outer: Loop2 { segs },
                    holes: vec![],
                };
                let builds = curve_on_frame(&k, &xy(), 0.0, &seg).is_ok();
                let checked = check_regions([&region].into_iter(), 1.0, tol);
                assert_eq!(
                    checked,
                    if builds {
                        Ok(())
                    } else {
                        Err(ProfileRefusal::LineOrArc)
                    },
                    "r {r}, sweep {sweep}"
                );
            }
        }
    }
}

/// A circle whose whole length is within the zone tolerance builds no arc, whatever the sweep:
/// at r = 1e-7 (length 6.3e-7 against 1e-5) a sweep of 101 rad clears `smallest_arc_sweep`
/// (100) and its end lies within the tolerance of a whole turn, but it is more than a turn,
/// which `Seg2::spans` and the conversion do not take. Refused by `curve_on_frame` and
/// `check_regions` alike, as is every sweep on that circle.
#[test]
fn an_arc_on_a_circle_within_the_zone_tolerance_is_refused() {
    let k = Kernel::default();
    let tol = k.tolerances.zone_tol_3d;
    assert_eq!(tol, ZONE);
    let r = 1e-7;
    for sweep in [101.0, -101.0, TAU, 1.0, 105.0] {
        let seg = Seg2::Arc {
            center: p(0.0, 0.0),
            radius: r,
            start: 0.5,
            sweep,
        };
        if sweep.abs() > 2.0 * TAU {
            assert!(seg.spans().is_none(), "{sweep}");
        }
        let c = curve_on_frame(&k, &xy(), 0.0, &seg);
        assert!(
            matches!(c, Err(crate::error::SolidError::InvalidInput { .. })),
            "{sweep}: {c:?}"
        );
        let region = Region2 {
            outer: Loop2 { segs: vec![seg] },
            holes: vec![],
        };
        assert_eq!(
            check_regions([&region].into_iter(), 1.0, tol),
            Err(ProfileRefusal::LineOrArc),
            "{sweep}"
        );
    }
}

/// The profile check takes the zone tolerance of the kernel that builds: at r = 1 a sweep of
/// 5e-6 (length 5e-6) is within the default 1e-5, refused by both, and over a zone tolerance of
/// 1e-7, built as an arc with its ends in closed form and accepted by the check, in a loop a line
/// closes exactly.
#[test]
fn the_profile_check_holds_the_building_kernels_zone_tolerance() {
    let seg = Seg2::Arc {
        center: p(0.0, 0.0),
        radius: 1.0,
        start: 0.5,
        sweep: 5e-6,
    };
    let region = Region2 {
        outer: Loop2 {
            segs: vec![
                seg,
                Seg2::Line {
                    a: seg.end(),
                    b: seg.start(),
                },
            ],
        },
        holes: vec![],
    };
    let k = Kernel::default();
    assert!(curve_on_frame(&k, &xy(), 0.0, &seg).is_err());
    assert_eq!(
        check_regions([&region].into_iter(), 1e-7, k.tolerances.zone_tol_3d),
        Err(ProfileRefusal::LineOrArc)
    );
    let mut k = Kernel::default();
    k.tolerances.zone_tol_3d = 1e-7;
    let c = curve_on_frame(&k, &xy(), 0.0, &seg).unwrap();
    let iv = c.natural_interval();
    let a = c.evaluate_point(&k, iv.min()).unwrap();
    let b = c.evaluate_point(&k, iv.max()).unwrap();
    let (s0, e0) = (
        on_circle(p(0.0, 0.0), 1.0, 0.5),
        on_circle(p(0.0, 0.0), 1.0, 0.5 + 5e-6),
    );
    assert!(
        off(a, s0) < ARC_CLOSED_FORM_TOL && off(b, e0) < ARC_CLOSED_FORM_TOL,
        "{a:?} {b:?}"
    );
    assert_eq!(
        check_regions([&region].into_iter(), 1e-7, k.tolerances.zone_tol_3d),
        Ok(())
    );
    // a zone tolerance that is not finite and positive refuses every arc
    for zone in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            check_regions([&region].into_iter(), 1e-7, zone),
            Err(ProfileRefusal::LineOrArc),
            "{zone}"
        );
    }
}

/// A polynomial quadratic (a font's quadratic curve: a conic of weight 1) reads back as that
/// conic, as a profile writes it, not as a degree-2 Bézier.
#[test]
fn a_polynomial_quadratic_reads_back_as_the_conic_of_weight_one() {
    let k = Kernel::default();
    let s = Seg2::Conic {
        a: p(0.0, 0.0),
        apex: p(1.0, 2.0),
        b: p(3.0, 0.5),
        w: 1.0,
    };
    let c = curve_on_frame(&k, &xy(), 0.0, &s).unwrap();
    let iv = c.natural_interval();
    for forward in [true, false] {
        let back = read(&k, &c, iv, forward, &xy(), 1e-7).unwrap();
        let [Seg2::Conic { a, apex, b, w }] = back[..] else {
            panic!("{back:?}")
        };
        let (a0, b0) = if forward {
            (s.start(), s.end())
        } else {
            (s.end(), s.start())
        };
        assert!(a.distance(a0) < 1e-12 && b.distance(b0) < 1e-12, "{back:?}");
        assert!(apex.distance(p(1.0, 2.0)) < 1e-12 && w == 1.0, "{back:?}");
    }
    // the same parabola as a quadratic Bézier made a curve: the same conic back
    let mut q = [p(0.0, 0.0); 10];
    q[1] = p(1.0, 2.0);
    q[2] = p(3.0, 0.5);
    let c = curve_on_frame(&k, &xy(), 0.0, &Seg2::Bezier { n: 2, p: q }).unwrap();
    let back = read(&k, &c, c.natural_interval(), true, &xy(), 1e-7).unwrap();
    assert!(
        matches!(back[..], [Seg2::Conic { w, .. }] if w == 1.0),
        "{back:?}"
    );
}

/// A curve moved off its definition is refused: a circle's net moved off the circle, a curve
/// lifted off the frame's plane at one end, a run's end moved off the curve's.
#[test]
fn a_moved_input_is_refused() {
    let k = Kernel::default();
    let w = FRAC_1_SQRT_2;
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let (quarter, iv) = spline(
        &k,
        knots.clone(),
        vec![
            [10.0, 0.0, 0.0, 1.0],
            [10.0 * w, 10.0 * w, 0.0, w],
            [0.0, 10.0, 0.0, 1.0],
        ],
    );
    for forward in [true, false] {
        let r = read(&k, &quarter, iv, forward, &xy(), 1e-5).unwrap();
        let Seg2::Arc {
            center,
            radius,
            sweep,
            ..
        } = r[0]
        else {
            panic!("{r:?}")
        };
        assert!(
            center.length() < 1e-8
                && (radius - 10.0).abs() < 1e-8
                && (sweep.abs() - FRAC_PI_2).abs() < 1e-8
        );
        assert_eq!(sweep > 0.0, forward);
    }
    // its middle control point moved 1e-4 off the circle: no arc, the conic it is
    let (moved, iv2) = spline(
        &k,
        knots.clone(),
        vec![
            [10.0, 0.0, 0.0, 1.0],
            [10.0 * w + 1e-4, 10.0 * w, 0.0, w],
            [0.0, 10.0, 0.0, 1.0],
        ],
    );
    let r = read(&k, &moved, iv2, true, &xy(), 1e-7).unwrap();
    assert!(matches!(r[..], [Seg2::Conic { .. }]), "{r:?}");
    // lifted 1e-3 at its end: off the plane, refused
    let (lifted, iv3) = spline(
        &k,
        knots,
        vec![
            [10.0, 0.0, 0.0, 1.0],
            [10.0 * w, 10.0 * w, 0.0, w],
            [0.0, 10.0, 1e-3, 1.0],
        ],
    );
    assert!(read(&k, &lifted, iv3, true, &xy(), 1e-5).is_none());
    // the run's end 1e-3 off the curve's: refused
    let mut r0 = run(&k, &quarter, iv, true);
    r0.ends[1].x += 1e-3;
    assert!(planar_pieces(&k, &r0, &xy(), 1e-5, &mut 2_000_000, false).is_none());
    // no work left: refused
    assert!(planar_pieces(&k, &run(&k, &quarter, iv, true), &xy(), 1e-5, &mut 0, false).is_none());
}

/// #17707: a few hundredths of a millimetre of a circle or an ellipse bends off its chord by
/// about 1e-5. That is under the tolerance a part's edges are read with (1e-6 of its size,
/// 7.7e-5 for the report's part) but over the 1e-5 an edge may lie off its face; read as a line,
/// it gave an Extrude To Object prism a flat side off the cylinder it lies on. It is read as an
/// arc (within the reading tolerance, the ellipse's as its circle of curvature); a rational piece
/// within the zone tolerance of its chord, or on it, stays a line.
#[test]
fn a_short_piece_of_a_circle_or_an_ellipse_is_not_a_line() {
    let k = Kernel::default();
    let tol = 7.7e-5;
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    // a rational quadratic piece of the ellipse (a cos t, b sin t) from t0 to t0 + 2h
    let piece = |a: f64, b: f64, t0: f64, h: f64| {
        let (mid, t1) = (t0 + h, t0 + 2.0 * h);
        spline(
            &k,
            knots.clone(),
            vec![
                [a * t0.cos(), b * t0.sin(), 0.0, 1.0],
                [a * mid.cos(), b * mid.sin(), 0.0, h.cos()],
                [a * t1.cos(), b * t1.sin(), 0.0, 1.0],
            ],
        )
    };
    // 0.036 of a circle of radius 15 (its sagitta 1.08e-5), and 0.08 of an ellipse of
    // semi-axes 30 and 15 about the end of its minor axis, where its radius of curvature is 60
    // (its sagitta 1.33e-5)
    let (h, g) = (0.0012, 0.04 / 30.0);
    let cases = [
        (piece(15.0, 15.0, 0.0, h), p(15.0 * h.cos(), 15.0 * h.sin())),
        (piece(30.0, 15.0, FRAC_PI_2 - g, g), p(0.0, 15.0)),
    ];
    for ((c, iv), mid) in cases {
        for forward in [true, false] {
            let r = read(&k, &c, iv, forward, &xy(), tol).unwrap();
            let [seg] = r.as_slice() else {
                panic!("one segment: {r:?}")
            };
            assert!(!matches!(seg, Seg2::Line { .. }), "read as a line: {seg:?}");
            assert!(
                seg.point_at(0.5).distance(mid) < 1e-9,
                "{seg:?} misses the curve's middle {mid:?}"
            );
        }
    }
    // a rational quadratic whose middle control point lies on its chord, or 2e-7 off it (well
    // within the 1e-5 zone tolerance; as an arc, its radius would be some 190 km), is that line
    for off in [0.0, 2e-7] {
        let (line, iv) = spline(
            &k,
            knots.clone(),
            vec![
                [0.0, 0.0, 0.0, 1.0],
                [2.5, 0.5 * off, 0.0, 0.5],
                [10.0, 0.0, 0.0, 1.0],
            ],
        );
        let r = read(&k, &line, iv, true, &xy(), tol).unwrap();
        assert!(matches!(r[..], [Seg2::Line { .. }]), "{off}: {r:?}");
    }
    // 80 of a circle of radius 4e7 is rational though its middle weight is 1 - 5e-13: its middle
    // control point lies 4e-5 off the chord, the arc 2e-5 (over the zone tolerance); the same
    // control points with equal weights (a polynomial) are not
    let (radius, h) = (4e7, 1e-6_f64);
    let (w, y) = (h.cos(), radius * h.cos() - radius);
    let (x, top) = (radius * h.sin(), radius / w - radius);
    let piece = |w: f64, v: f64| {
        vec![vec![
            [-x * v, y * v, v],
            [0.0, top * w, w],
            [x * v, y * v, v],
        ]]
    };
    assert!(arc_before_line(&piece(w, 1.0), 1e-5));
    assert!(!arc_before_line(&piece(w, 1.0), 5e-5));
    assert!(!arc_before_line(&piece(1.0, 1.0), 1e-5));
    assert!(!arc_before_line(&piece(0.5, 0.5), 1e-5));
    // a piece of a circle of radius 1e8 (half-angle 6.3e-7, middle weight 1 - 2e-13, bending
    // 2e-5 off its chord) raised to degree 32: neighbouring weights differ by at most 1.3e-14,
    // under their rounding, the span's by 1e-13; it is rational
    let (radius, h) = (1e8, 4e-13_f64.sqrt());
    let (w, y) = (h.cos(), radius * h.cos() - radius);
    let x = radius * h.sin();
    let mut span = vec![
        [-x, y, 1.0],
        [0.0, (radius / w - radius) * w, w],
        [x, y, 1.0],
    ];
    for n in 2..32 {
        let mut up = vec![span[0]];
        for i in 1..=n {
            let a = i as f64 / (n + 1) as f64;
            up.push(std::array::from_fn(|k| {
                a * span[i - 1][k] + (1.0 - a) * span[i][k]
            }));
        }
        up.push(span[n]);
        span = up;
    }
    assert_eq!(span.len(), 33);
    assert!(arc_before_line(&[span], 1e-5));
}

/// Samples do not certify: a C0 quadratic spline on the x axis at every knot k/16 and bowing 1
/// off it in its first span is no line; a cubic retracing its line is none; a curve twice round
/// a circle is no arc.
#[test]
fn sampling_and_retracing_do_not_certify() {
    let k = Kernel::default();
    let mut knots = vec![0.0; 3];
    let mut pts: Vec<[f64; 4]> = vec![[0.0, 0.0, 0.0, 1.0]];
    for i in 0..16 {
        let (a, b) = (f64::from(i) / 16.0, f64::from(i + 1) / 16.0);
        let bump = if i == 0 { 2.0 } else { 0.0 };
        pts.push([10.0 * (a + b) * 0.5, bump, 0.0, 1.0]);
        pts.push([10.0 * b, 0.0, 0.0, 1.0]);
        knots.extend([b, b]);
    }
    knots.push(1.0);
    let (bowed, iv) = spline(&k, knots, pts);
    let r = read(&k, &bowed, iv, true, &xy(), 1e-3);
    assert!(!matches!(r.as_deref(), Some([Seg2::Line { .. }])), "{r:?}");
    let back: Vec<[f64; 4]> = [0.0, 10.0, -35.0, 35.0]
        .iter()
        .map(|x| [*x, 0.0, 0.0, 1.0])
        .collect();
    let (back, iv) = spline(&k, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], back);
    assert!(read(&k, &back, iv, true, &xy(), 1e-5).is_none());
    let w = FRAC_1_SQRT_2;
    let mut knots = vec![0.0; 3];
    let mut pts = vec![[10.0, 0.0, 0.0, 1.0]];
    for i in 0..8 {
        let a = f64::from(i) * FRAC_PI_2;
        let (m, b) = (a + FRAC_PI_2 * 0.5, a + FRAC_PI_2);
        pts.push([10.0 * m.cos() / w * w, 10.0 * m.sin() / w * w, 0.0, w]);
        pts.push([10.0 * b.cos(), 10.0 * b.sin(), 0.0, 1.0]);
        knots.extend([f64::from(i + 1) / 8.0; 2]);
    }
    knots.push(1.0);
    let (twice, iv) = spline(&k, knots, pts);
    assert!(read(&k, &twice, iv, true, &xy(), 1e-5).is_none());
}

/// A curve near a circle (a parabola) is kept as the curve it is, its ends exact, at either
/// tolerance: an arc ending a micron off would open a profile loop.
#[test]
fn a_curve_near_a_circle_is_not_an_arc_ending_off_it() {
    let k = Kernel::default();
    let (e, iv) = spline(
        &k,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            [0.0, 0.0, 0.0, 1.0],
            [2.0, 0.0, 0.0, 1.0],
            [2.0, 0.2, 0.0, 1.0],
        ],
    );
    for tol in [1e-5, 2e-3] {
        let segs = read(&k, &e, iv, true, &xy(), tol).unwrap();
        let (f, l) = (segs.first().unwrap(), segs.last().unwrap());
        assert!(
            f.start().distance(p(0.0, 0.0)) < 1e-9 && l.end().distance(p(2.0, 0.2)) < 1e-9,
            "{tol}: {segs:?}"
        );
    }
}

/// A circular reading keeps the arc only when each end it returns lies within the tolerance of its
/// vertex: a vertex just inside is read, one just outside is refused (the edge stays a spline),
/// and one exactly on the circle is read.
#[test]
fn a_circular_reading_ends_within_tolerance_of_its_vertices() {
    let k = Kernel::default();
    let w = FRAC_1_SQRT_2;
    // the quarter of the unit circle from (1, 0) to (0, 1)
    let (c, iv) = spline(
        &k,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![[1.0, 0.0, 0.0, 1.0], [w, w, 0.0, w], [0.0, 1.0, 0.0, 1.0]],
    );
    let tol = 1e-6;
    let read_at = |first_x: f64, last_y: f64| {
        let run = Run {
            curve: &c,
            interval: iv,
            forward: true,
            ends: [
                crate::geom::vector3d::Vec3::new(first_x, 0.0, 0.0),
                crate::geom::vector3d::Vec3::new(0.0, last_y, 0.0),
            ],
        };
        circular_boundary(&k, &run, &xy(), p(0.0, 0.0), 1.0, tol, &mut 2_000_000)
    };
    assert!(read_at(1.0, 1.0).is_some());
    assert!(read_at(1.0 + 0.9 * tol, 1.0).is_some());
    assert!(read_at(1.0, 1.0 - 0.9 * tol).is_some());
    assert!(read_at(1.0 + 1.1 * tol, 1.0).is_none());
    assert!(read_at(1.0, 1.0 + 1.1 * tol).is_none());
}

/// A spline of several spans reads as the one B-spline it is for a sketch, run back, its ends
/// exact.
#[test]
fn a_spline_of_several_spans_is_one_planar_spline() {
    let k = Kernel::default();
    let pts = [[0.0, 0.0], [1.0, 2.0], [2.0, -1.0], [3.0, 2.0], [4.0, 0.0]]
        .map(|q| [q[0], q[1], 0.0, 1.0]);
    let (c, iv) = spline(
        &k,
        vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        pts.to_vec(),
    );
    let r = planar_curves(&k, &run(&k, &c, iv, false), &xy(), 1e-7, &mut 2_000_000).unwrap();
    let [PlanarPiece::Spline(s)] = &r[..] else {
        panic!("{r:?}")
    };
    assert_eq!(s.degree, 3);
    assert!(s.weights.is_empty());
    assert!(
        s.points[0].distance(p(4.0, 0.0)) < 1e-12
            && s.points.last().unwrap().distance(p(0.0, 0.0)) < 1e-12
    );
}

fn combine(k: &mut Kernel, a: Brep, b: Brep, op: Boolean2d) -> Option<Brep> {
    match boolean_2d(k, a, b, op).unwrap() {
        Boolean2dOutcome::Result(r) => Some(r),
        Boolean2dOutcome::Unassigned { .. } => None,
    }
}

fn sheet(k: &mut Kernel, region: &Region2, tol: f64) -> Brep {
    let lp = region_loops(region, tol).unwrap();
    assert!(lp.holes.is_empty(), "test sheets have no holes");
    let mut faces = Vec::new();
    for l in &lp.material {
        let curves: Vec<Curve> = l
            .segs
            .iter()
            .map(|s| curve_on_frame(k, &xy(), 0.0, s).unwrap())
            .collect();
        faces.push(create_planar_faces(k, &curves).unwrap());
    }
    let mut it = faces.into_iter();
    let mut acc = it.next().unwrap();
    for f in it {
        acc = combine(k, acc, f, Boolean2d::Union).unwrap();
    }
    acc
}

#[test]
fn sheets_combined_read_back_as_their_regions() {
    let mut k = Kernel::default();
    let a = sheet(&mut k, &rectangle(p(0.0, 0.0), p(10.0, 10.0)), 1e-7);
    let b = sheet(&mut k, &rectangle(p(5.0, 0.0), p(15.0, 10.0)), 1e-7);
    let u = combine(&mut k, a, b, Boolean2d::Union).unwrap();
    let r = sheet_regions(&k, &u, &xy(), 1e-7).unwrap();
    assert_eq!(r.regions.len(), 1, "{r:?}");
    assert!(!r.touching);
    let area = r.regions[0].outer.signed_area().unwrap();
    assert!((area.value - 150.0).abs() <= 1e-9, "{area:?}");
    // a disc cut out of a plate: a hole of exact arcs, clockwise
    let plate = sheet(&mut k, &rectangle(p(0.0, 0.0), p(20.0, 20.0)), 1e-7);
    let disc = sheet(
        &mut k,
        &Region2 {
            outer: circle(p(10.0, 10.0), 6.0),
            holes: vec![],
        },
        1e-7,
    );
    let cut = combine(&mut k, plate, disc, Boolean2d::Difference).unwrap();
    let r = sheet_regions(&k, &cut, &xy(), 1e-7).unwrap();
    assert_eq!(r.regions.len(), 1, "{r:?}");
    assert_eq!(r.regions[0].holes.len(), 1, "{r:?}");
    let hole = r.regions[0].holes[0].signed_area().unwrap();
    assert!((hole.value + 36.0 * PI).abs() <= 1e-9, "{hole:?}");
    assert!(r.regions[0].holes[0]
        .segs
        .iter()
        .all(|s| matches!(s, Seg2::Arc { .. })));
}

/// A disc's sheet read in a frame and in the same frame turned over (its y axis reversed, so
/// the face is seen from below in one of them): a face seen from below has its loops reversed,
/// each arc then starting at its old end, `start + sweep`, past π or under -π for one of the
/// disc's two half-turns. Read either way, every arc starts at an angle in (-π, π], at the
/// point the angle names on the circle, and the disc's area is its closed form.
#[test]
fn a_sheet_seen_from_below_starts_its_arcs_in_range() {
    let mut k = Kernel::default();
    let (c, r) = (p(3.0, -2.0), 4.0);
    let disc = sheet(
        &mut k,
        &Region2 {
            outer: circle(c, r),
            holes: vec![],
        },
        1e-7,
    );
    let below = Frame {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, -1.0, 0.0],
    };
    for (frame, centre) in [(xy(), c), (below, p(c.x, -c.y))] {
        let got = sheet_regions(&k, &disc, &frame, 1e-7).unwrap();
        assert_eq!(got.regions.len(), 1, "{got:?}");
        let outer = &got.regions[0].outer;
        let area = outer.signed_area().unwrap();
        assert!((area.value - PI * r * r).abs() <= area.bound, "{area:?}");
        assert_eq!(outer.segs.len(), 2, "{outer:?}");
        for s in &outer.segs {
            let Seg2::Arc {
                center,
                radius,
                start,
                ..
            } = *s
            else {
                panic!("not an arc: {s:?}");
            };
            assert!(start > -PI && start <= PI, "{start}");
            assert!(center.distance(centre) < 1e-12 && (radius - r).abs() < 1e-12);
        }
        // the loop still closes: each arc starts where the one before ends
        for (a, b) in outer.segs.iter().zip(outer.segs.iter().cycle().skip(1)) {
            assert!(a.end().distance(b.start()) < 1e-12, "{outer:?}");
        }
    }
}

fn drilled(k: &mut Kernel, radius: f64, shift: f64) -> Brep {
    let region = Region2 {
        outer: polygon(&[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0), p(0.0, 20.0)]),
        holes: vec![circle(p(10.0 + shift, 10.0), radius).reversed()],
    };
    extrude_region(k, &xy(), &region, 0.0, 10.0).unwrap()
}

#[test]
fn a_drilled_prism_has_its_section_and_its_volume_enclosed() {
    let mut k = Kernel::default();
    let source = drilled(&mut k, 6.0, 0.0);
    let s = section(&k, &source, &xy()).expect("a drilled prism");
    let exact = 4000.0 - 360.0 * PI;
    assert!(
        s.volume.lo <= exact && s.volume.hi >= exact && s.volume.hi - s.volume.lo <= 1e-3,
        "{:?}",
        s.volume
    );
    assert!(
        s.lo.abs() <= s.tolerance && (s.hi - 10.0).abs() <= s.tolerance,
        "{s:?}"
    );
    assert_eq!(s.region.holes.len(), 1);
    let area = s.region.outer.signed_area().unwrap().value
        + s.region.holes[0].signed_area().unwrap().value;
    assert!((area - (400.0 - 36.0 * PI)).abs() < 1e-8, "{area}");
    // along another direction it is not proved a prism
    let side = Frame {
        origin: [0.0; 3],
        x_axis: [0.0, 1.0, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    };
    assert!(section(&k, &source, &side).is_none());
    let same = drilled(&mut k, 6.0, 0.0);
    assert_eq!(equivalent(&k, &source, &same), Some(true));
    let wider = drilled(&mut k, 6.0001, 0.0);
    assert_eq!(equivalent(&k, &source, &wider), Some(false));
    let shifted = drilled(&mut k, 6.0, 0.0001);
    assert_eq!(equivalent(&k, &source, &shifted), Some(false));
}

#[test]
fn a_box_is_a_prism_along_each_of_its_axes() {
    let mut k = Kernel::default();
    let b = extrude_region(
        &mut k,
        &xy(),
        &rectangle(p(0.0, 0.0), p(4.0, 3.0)),
        -1.0,
        2.0,
    )
    .unwrap();
    let s = section(&k, &b, &xy()).unwrap();
    assert!(
        (s.lo + 1.0).abs() <= s.tolerance && (s.hi - 2.0).abs() <= s.tolerance,
        "{s:?}"
    );
    let side = Frame {
        origin: [0.0; 3],
        x_axis: [0.0, 1.0, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    };
    let s = section(&k, &b, &side).unwrap();
    assert!(
        s.lo.abs() <= s.tolerance && (s.hi - 4.0).abs() <= s.tolerance,
        "{s:?}"
    );
}

/// A spline that jumps (a knot past its degree) is refused where it is made ([`rational_curve`]
/// never lets the constructor repair it into another curve), or not read.
#[test]
fn discontinuous_splines_are_not_read() {
    let k = Kernel::default();
    let w = FRAC_1_SQRT_2;
    let cases: [(Vec<f64>, Vec<[f64; 4]>); 2] = [
        (
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0],
            vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
                [-10.0, 0.0, 0.0, 1.0],
                [-10.0 * w, -10.0 * w, 0.0, w],
                [0.0, -10.0, 0.0, 1.0],
            ],
        ),
        (
            vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0],
            [0.0, 2.0, 5.0, 10.0]
                .iter()
                .map(|x| [*x, 0.0, 0.0, 1.0])
                .collect(),
        ),
    ];
    let mut refused = 0;
    for (knots, homogeneous) in cases {
        let Ok(c) = rational_curve(&knots, &homogeneous) else {
            refused += 1;
            continue;
        };
        assert!(read(&k, &c, c.natural_interval(), true, &xy(), 1e-5).is_none());
    }
    assert_eq!(refused, 2, "both jumps refused where they are made");
}

#[test]
fn a_tapered_flange_changes_by_half_its_length_times_the_slope_tangent() {
    // No slope: the given thickness at both ends.
    assert_eq!(tapered_flange_ends(80.0, 14.0, 0.0), (14.0, 14.0));
    // 45 degrees over an 80 mm inside length: 40 mm thinner at the toe, 40 mm thicker at the root.
    let (toe, root) = tapered_flange_ends(80.0, 50.0, 45.0);
    assert!(
        (toe - 10.0).abs() < 1e-12 && (root - 90.0).abs() < 1e-12,
        "{toe} {root}"
    );
    // A 5 degree taper flange channel (203 x 89, web 8.9, flange 14.2): the ends are the mean
    // thickness less and plus 40.05 tan 5°, and they average back to it.
    let (toe, root) = tapered_flange_ends(89.0 - 8.9, 14.2, 5.0);
    let change = 40.05 * 5.0_f64.to_radians().tan();
    assert!((toe - (14.2 - change)).abs() < 1e-12 && (root - (14.2 + change)).abs() < 1e-12);
    assert!(((toe + root) / 2.0 - 14.2).abs() < 1e-12);
    // A non-finite input gives a non-finite answer, never a panic.
    assert!(tapered_flange_ends(f64::NAN, 1.0, 5.0).0.is_nan());
}

#[test]
fn directed_planar_sweep_keeps_quadrants_and_tiny_seam_angles() {
    let sweep = super::profile::counterclockwise_sweep;
    let center = Point2::new(0.0, 0.0);
    let x = Point2::new(1.0, 0.0);
    let y = Point2::new(0.0, 1.0);
    assert_eq!(sweep(center, x, y), Some(FRAC_PI_2));
    assert_eq!(sweep(center, y, x), Some(3.0 * FRAC_PI_2));
    assert_eq!(sweep(center, x, x), Some(0.0));
    for delta in [0.5e-12_f64, 1e-12, 2e-12] {
        let end = Point2::new(delta.cos(), delta.sin());
        assert_eq!(sweep(center, x, end), Some(delta));
        let start = Point2::new(-1.0, delta / 2.0);
        let end = Point2::new(-1.0, -delta / 2.0);
        let expected = (end.y.atan2(end.x) - start.y.atan2(start.x)).rem_euclid(TAU);
        assert_eq!(sweep(center, start, end), Some(expected));
        assert!(expected > 0.0 && expected < 3e-12);
    }
    assert_eq!(sweep(center, center, x), None);
    assert_eq!(sweep(center, x, Point2::new(f64::NAN, 0.0)), None);
}

mod recognition {
    use super::*;
    use crate::geom::certified::{next_down, next_up};
    use crate::prism::recognition::{
        conic_ellipse, loop_circle, loop_ellipse, EllipseArc, RecognitionVector,
    };
    trait RecognizedLoop {
        fn as_circle(&self) -> Option<(Vec2, f64)>;
        fn as_ellipse(&self) -> Option<(Vec2, Vec2, f64)>;
        fn circle(center: Vec2, radius: f64) -> Self;
        fn circle_from(center: Vec2, radius: f64, start: f64) -> Self;
    }
    impl RecognizedLoop for Loop2 {
        fn as_circle(&self) -> Option<(Vec2, f64)> {
            loop_circle(self)
        }
        fn as_ellipse(&self) -> Option<(Vec2, Vec2, f64)> {
            loop_ellipse(self)
        }
        fn circle(center: Vec2, radius: f64) -> Self {
            Self::circle_from(center, radius, 0.0)
        }
        fn circle_from(center: Vec2, radius: f64, start: f64) -> Self {
            Self {
                segs: vec![Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep: TAU,
                }],
            }
        }
    }
    #[test]
    fn profile_circle_recognition_tracks_direction_and_one_turn() {
        use std::f64::consts::TAU;
        for (center, radius) in [
            (Vec2::ZERO, 2.0),
            (Vec2::ZERO, 1e12),
            (Vec2::new(1e12, -1e12), 1e9),
        ] {
            for count in [1, 2, 3, 4] {
                for start in [0.0, 0.1, 0.3, 2.7, 100.0, -100.0] {
                    let sweep = TAU / f64::from(count);
                    let circle = Loop2 {
                        segs: (0..count)
                            .map(|i| Seg2::Arc {
                                center,
                                radius,
                                start: start + f64::from(i) * sweep,
                                sweep,
                            })
                            .collect(),
                    };
                    for mut path in [circle.clone(), circle.reversed()] {
                        for _ in 0..count {
                            assert_eq!(
                                path.as_circle(),
                                Some((center, radius)),
                                "count={count}, start={start}, {path:?}"
                            );
                            path.segs.rotate_left(1);
                        }
                    }
                }
            }
        }
        let circle = Loop2::circle_from(Vec2::new(1e12, -1e12), 1e9, 0.3);
        assert_eq!(circle.as_circle(), Some((Vec2::new(1e12, -1e12), 1e9)));
        assert_eq!(circle.reversed().as_circle(), circle.as_circle());
        // Each piece may store an equivalent phase from a different turn.
        let mut circle = Loop2 {
            segs: vec![
                Seg2::Arc {
                    center: Vec2::ZERO,
                    radius: 2.0,
                    start: 0.0,
                    sweep: TAU * 0.5,
                },
                Seg2::Arc {
                    center: Vec2::ZERO,
                    radius: 2.0,
                    start: TAU * 0.5,
                    sweep: TAU * 0.5,
                },
            ],
        };
        if let Seg2::Arc { start, .. } = &mut circle.segs[1] {
            *start = 3.0 * PI;
        }
        for mut path in [circle.clone(), circle.reversed()] {
            for _ in 0..2 {
                assert_eq!(path.as_circle(), Some((Vec2::ZERO, 2.0)));
                path.segs.rotate_left(1);
            }
        }
    }

    #[test]
    fn profile_circle_recognition_rejects_gaps_retracing_and_invalid_arcs() {
        use std::f64::consts::{PI, TAU};
        let arc = |start, sweep| Seg2::Arc {
            center: Vec2::ZERO,
            radius: 2.0,
            start,
            sweep,
        };
        for segs in [
            vec![],
            vec![arc(0.0, PI)],
            vec![arc(0.0, PI), arc(0.0, PI)],
            vec![arc(0.0, PI), arc(PI + 0.01, PI)],
            vec![arc(0.0, PI), arc(PI + 1e-8, PI)],
            vec![arc(0.0, PI), arc(PI, -PI)],
            vec![arc(0.0, PI), arc(PI, PI), arc(TAU, PI), arc(TAU + PI, PI)],
            vec![arc(0.0, TAU), arc(0.0, TAU)],
            vec![
                arc(0.0, PI),
                Seg2::Line {
                    a: Vec2::new(-2.0, 0.0),
                    b: Vec2::new(2.0, 0.0),
                },
            ],
        ] {
            let path = Loop2 { segs };
            assert_eq!(path.as_circle(), None, "{path:?}");
            assert_eq!(path.reversed().as_circle(), None, "{path:?}");
        }
        for sweep in [
            0.0,
            TAU + 1e-13,
            -TAU - 1e-13,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(
                Loop2 {
                    segs: vec![arc(0.0, sweep)]
                }
                .as_circle(),
                None
            );
        }
        for start in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e20, f64::MAX] {
            assert_eq!(Loop2::circle_from(Vec2::ZERO, 2.0, start).as_circle(), None);
        }
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(Loop2::circle(Vec2::ZERO, radius).as_circle(), None);
        }
        assert_eq!(
            Loop2::circle(Vec2::new(f64::NAN, 0.0), 2.0).as_circle(),
            None
        );
        let mut path = Loop2 {
            segs: vec![arc(0.0, PI), arc(PI, PI)],
        };
        path.segs[1] = Seg2::Arc {
            center: Vec2::ZERO,
            radius: next_up(2.0_f64),
            start: PI,
            sweep: PI,
        };
        assert_eq!(path.as_circle(), None, "support equality is exact");
        path.segs[1] = Seg2::Arc {
            center: Vec2::new(1e-15, 0.0),
            radius: 2.0,
            start: PI,
            sweep: PI,
        };
        assert_eq!(path.as_circle(), None);
        // The stored phase resolves an extra 0.125 radians; its rounding cell must not
        // close the resulting gap/overlap by independently widening both arc endpoints.
        let start = 2.0_f64.powi(49);
        let circle = Loop2 {
            segs: vec![arc(start, PI), arc((start + PI) + 0.125, PI)],
        };
        for mut path in [circle.clone(), circle.reversed()] {
            for _ in 0..2 {
                assert_eq!(path.as_circle(), None, "{path:?}");
                path.segs.rotate_left(1);
            }
        }
    }

    fn ellipse_quarters(center: Vec2, u: Vec2, v: Vec2) -> Loop2 {
        let axes = [u, v, -u, -v];
        Loop2 {
            segs: (0..4)
                .map(|i| Seg2::Conic {
                    a: center + axes[i],
                    apex: (center + axes[i]) + axes[(i + 1) % 4],
                    b: center + axes[(i + 1) % 4],
                    w: FRAC_1_SQRT_2,
                })
                .collect(),
        }
    }

    #[test]
    fn profile_ellipse_recognition_preserves_quarters_and_orientation() {
        for (center, u, v) in [
            (
                Vec2::new(3.0, -2.0),
                Vec2::new(5.0, 0.0),
                Vec2::new(0.0, 2.0),
            ),
            (
                Vec2::new(17.0, -11.0),
                Vec2::new(12.0, 9.0),
                Vec2::new(-4.8, 6.4),
            ),
        ] {
            let ellipse = ellipse_quarters(center, u, v);
            for mut path in [ellipse.clone(), ellipse.reversed()] {
                for _ in 0..4 {
                    let (c, major, ratio) =
                        path.as_ellipse().expect("four original ellipse quarters");
                    assert_eq!(c, center);
                    assert!(
                        (major.length_hypot() - u.length_hypot()).abs() < 1e-12,
                        "{major:?}"
                    );
                    assert!(
                        (ratio - v.length_hypot() / u.length_hypot()).abs() < 1e-12,
                        "{ratio}"
                    );
                    for seg in &path.segs {
                        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                            let d = seg.point_at(t) - c;
                            let x = d.dot(major) / major.length_squared();
                            let y = d.dot(major.perp()) / (major.length_squared() * ratio);
                            assert!((x * x + y * y - 1.0).abs() < 1e-12);
                        }
                    }
                    path.segs.rotate_left(1);
                }
            }
        }
    }

    #[test]
    fn profile_ellipse_recognition_does_not_replace_other_paths() {
        let ellipse = ellipse_quarters(
            Vec2::new(3.0, -2.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(0.0, 2.0),
        );
        let q = ellipse.segs[0];
        assert_eq!(
            Loop2 {
                segs: vec![q, q.reversed(), q, q.reversed()]
            }
            .as_ellipse(),
            None
        );
        assert_eq!(Loop2 { segs: vec![q; 4] }.as_ellipse(), None);
        assert_eq!(
            Loop2 {
                segs: ellipse.segs.repeat(2)
            }
            .as_ellipse(),
            None
        );
        assert_eq!(
            Loop2 {
                segs: ellipse.segs[..3].to_vec()
            }
            .as_ellipse(),
            None
        );
        assert_eq!(Loop2::circle(Vec2::ZERO, 2.0).as_ellipse(), None);
        assert_eq!(Loop2::default().as_ellipse(), None);
        for delta in [Vec2::new(0.001, 0.0), Vec2::new(f64::NAN, 0.0)] {
            let mut path = ellipse.clone();
            if let Seg2::Conic { a, .. } = &mut path.segs[1] {
                *a += delta;
            }
            assert_eq!(path.as_ellipse(), None);
        }
        for weight in [0.5, 1.0, 1.1, f64::NAN, f64::INFINITY] {
            let mut path = ellipse.clone();
            if let Seg2::Conic { w, .. } = &mut path.segs[1] {
                *w = weight;
            }
            assert_eq!(path.as_ellipse(), None);
        }
        // The world-coordinate midpoint rounds to c, but the x displacements are +4/-3
        // or +4/-3.5. Even when one offset reconstructs both rounded endpoints, the local
        // asymmetry is significant and must not be replaced by a full ellipse.
        let c = 2.0_f64.powi(52);
        for left in [3.0, 3.5] {
            let points = [
                Vec2::new(c + 4.0, 0.0),
                Vec2::new(c, 2.0),
                Vec2::new(c - left, 0.0),
                Vec2::new(c, -2.0),
            ];
            let apices = [
                Vec2::new(c + 4.0, 2.0),
                Vec2::new(c - left, 2.0),
                Vec2::new(c - left, -2.0),
                Vec2::new(c + 4.0, -2.0),
            ];
            let ellipse = Loop2 {
                segs: (0..4)
                    .map(|i| Seg2::Conic {
                        a: points[i],
                        apex: apices[i],
                        b: points[(i + 1) % 4],
                        w: FRAC_1_SQRT_2,
                    })
                    .collect(),
            };
            for mut path in [ellipse.clone(), ellipse.reversed()] {
                for _ in 0..4 {
                    assert_eq!(path.as_ellipse(), None, "{path:?}");
                    path.segs.rotate_left(1);
                }
            }
        }
    }

    #[test]
    fn profile_conic_ellipse_keeps_directed_endpoints_and_unclamped_ratio() {
        let ellipse = ellipse_quarters(
            Vec2::new(3.0, -2.0),
            Vec2::new(5.0, 0.0),
            Vec2::new(0.0, 2.0),
        );
        for seg in &ellipse.segs {
            for (piece, positive) in [(*seg, true), (seg.reversed(), false)] {
                let Seg2::Conic { a, apex, b, w } = piece else {
                    panic!("fixture must be a conic")
                };
                let arc = conic_ellipse(a, apex, b, w).expect("elliptical quarter");
                assert_eq!(arc.end > arc.start, positive);
                for (t, expected) in [
                    (arc.start, a),
                    ((arc.start + arc.end) * 0.5, piece.point_at(0.5)),
                    (arc.end, b),
                ] {
                    let actual =
                        arc.center + arc.major * t.cos() + arc.major.perp() * (arc.ratio * t.sin());
                    assert!(
                        actual.dist(expected) < 1e-12,
                        "{piece:?}: {actual:?} != {expected:?}"
                    );
                }
            }
        }
        let thin = ellipse_quarters(Vec2::ZERO, Vec2::new(1e12, 0.0), Vec2::new(0.0, 0.5));
        let (_, _, ratio) = thin
            .as_ellipse()
            .expect("thin ellipse from its original axes");
        assert!((ratio - 5e-13).abs() < 1e-25);
        for seg in thin.segs {
            let Seg2::Conic { a, apex, b, w } = seg else {
                panic!("fixture must be a conic")
            };
            if let Some(arc) = conic_ellipse(a, apex, b, w) {
                assert!(arc.ratio < 1e-12, "a small ratio must not be clamped");
            }
        }
        for w in [0.2, 0.5, 0.8] {
            assert!(conic_ellipse(
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
                w
            )
            .is_some());
        }
        for w in [-1.0, 0.0, 1.0 - 1e-7, 1.0, 1.1, f64::NAN, f64::INFINITY] {
            assert!(conic_ellipse(
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
                w
            )
            .is_none());
        }
        assert!(conic_ellipse(Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0), 0.5).is_none());
        assert!(conic_ellipse(Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, 0.5).is_none());
        assert!(conic_ellipse(
            Vec2::new(f64::INFINITY, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            0.5
        )
        .is_none());
    }

    #[test]
    fn profile_conic_ellipse_refuses_lost_local_displacements() {
        // The exact stored-input midpoint is (C+2.5,1), but the world sum rounds
        // it to (C+2,1). The old extraction then moves A from C+4 to C+3.
        let c = 2.0_f64.powi(52);
        for x_sign in [1.0, -1.0] {
            for reflect in [1.0, -1.0] {
                let a = Vec2::new(x_sign * (c + 4.0), 0.0);
                let apex = Vec2::new(x_sign * (c + 4.0), 2.0 * reflect);
                let b = Vec2::new(x_sign * (c + 1.0), 2.0 * reflect);
                for (a, apex, b) in [
                    (a, apex, b),
                    (
                        Vec2::new(a.y, a.x),
                        Vec2::new(apex.y, apex.x),
                        Vec2::new(b.y, b.x),
                    ),
                ] {
                    for (start, end) in [(a, b), (b, a)] {
                        assert!(conic_ellipse(start, apex, end, 0.5_f64.sqrt()).is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn profile_conic_ellipse_recovers_nonquarter_analytic_parameters() {
        // For A=(0,-2), P=(4,0), B=(0,2), eliminating the rational parameter gives
        // (1-w²)x²/(16w²) + x/2 + y²/4 = 1. Completing its square gives these independent
        // centers and principal axes. At w=1/5 the longer axis is Y, not X.
        let cases = [
            (
                0.2,
                Vec2::new(-1.0 / 6.0, 0.0),
                Vec2::new(0.0, 5.0 / 6.0_f64.sqrt()),
                1.0 / 6.0_f64.sqrt(),
                2.0 / 3.0,
            ),
            (
                0.5,
                Vec2::new(-4.0 / 3.0, 0.0),
                Vec2::new(8.0 / 3.0, 0.0),
                3.0_f64.sqrt() / 2.0,
                4.0 / 3.0,
            ),
            (
                0.6,
                Vec2::new(-9.0 / 4.0, 0.0),
                Vec2::new(15.0 / 4.0, 0.0),
                2.0 / 3.0,
                1.5,
            ),
            (
                0.8,
                Vec2::new(-64.0 / 9.0, 0.0),
                Vec2::new(80.0 / 9.0, 0.0),
                3.0 / 8.0,
                16.0 / 9.0,
            ),
        ];
        let (a, apex, b) = (
            Vec2::new(0.0, -2.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, 2.0),
        );
        for (w, center, major, ratio, middle_x) in cases {
            for (start, end, direction) in [(a, b, 1.0), (b, a, -1.0)] {
                let arc = conic_ellipse(start, apex, end, w).expect("non-quarter elliptic conic");
                assert_profile_ellipse_arc(
                    arc,
                    center,
                    major,
                    ratio,
                    direction * 2.0 * w.acos(),
                    [start, Vec2::new(middle_x, 0.0), end],
                );
            }
        }
    }

    fn assert_profile_ellipse_arc(
        arc: EllipseArc,
        center: Vec2,
        major: Vec2,
        ratio: f64,
        sweep: f64,
        points: [Vec2; 3],
    ) {
        assert!(
            arc.center.dist(center) < 1e-12,
            "center: {arc:?}, expected {center:?}"
        );
        // Changing the major-axis sign and shifting both parameters describes the same arc.
        let aligned_major = if arc.major.dot(major) < 0.0 {
            -arc.major
        } else {
            arc.major
        };
        assert!(
            aligned_major.dist(major) < 1e-12,
            "major: {arc:?}, expected ±{major:?}"
        );
        assert!(
            (arc.ratio - ratio).abs() < 1e-12,
            "ratio: {arc:?}, expected {ratio}"
        );
        assert!(
            (arc.end - arc.start - sweep).abs() < 1e-12,
            "sweep: {arc:?}, expected {sweep}"
        );
        // Only the symmetric rational midpoint corresponds to the angular midpoint. The
        // rational parameter is not generally a linear fraction of this angular interval.
        for (t, expected) in [arc.start, (arc.start + arc.end) * 0.5, arc.end]
            .into_iter()
            .zip(points)
        {
            let actual =
                arc.center + arc.major * t.cos() + arc.major.perp() * (arc.ratio * t.sin());
            assert!(
                actual.dist(expected) < 1e-12,
                "point: {actual:?}, expected {expected:?}, {arc:?}"
            );
        }
    }

    #[test]
    fn profile_conic_ellipse_preserves_transformed_partial_arcs() {
        // Apply T(x,y)=(7-2y,-5+2x) to the w=3/5 conic above: rotate 90°, scale 2,
        // translate. Its independent center is (7,-19/2), semiaxes 15/2 and 5.
        // Reflecting Y then reverses the orientation while keeping both axis lengths.
        for reflection in [1.0, -1.0] {
            let (a, apex, b) = (
                Vec2::new(11.0, -5.0 * reflection),
                Vec2::new(7.0, 3.0 * reflection),
                Vec2::new(3.0, -5.0 * reflection),
            );
            for (start, end, direction) in [(a, b, reflection), (b, a, -reflection)] {
                let arc = conic_ellipse(start, apex, end, 0.6)
                    .expect("transformed non-quarter elliptic conic");
                assert_profile_ellipse_arc(
                    arc,
                    Vec2::new(7.0, -9.5 * reflection),
                    Vec2::new(0.0, 7.5),
                    2.0 / 3.0,
                    direction * 2.0 * 0.6_f64.acos(),
                    [start, Vec2::new(7.0, -2.0 * reflection), end],
                );
            }
        }
    }

    #[test]
    fn profile_conic_ellipse_declines_nonelliptic_weights_and_conservative_boundaries() {
        let (a, apex, b) = (
            Vec2::new(0.0, -2.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, 2.0),
        );
        for w in [
            -f64::MAX,
            -1.0,
            -0.0,
            0.0,
            next_down(1e-9_f64),
            1e-9,
            1.0 - 1e-6,
            1.0 - 1e-7,
            next_down(1.0_f64),
            1.0,
            next_up(1.0_f64),
            1.5,
            f64::MAX,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(conic_ellipse(a, apex, b, w).is_none(), "w={w}");
            assert!(conic_ellipse(b, apex, a, w).is_none(), "reversed w={w}");
        }
        // Small positive weights and weights just below 1 still describe ellipses, but the
        // inherited extractor guards conservatively retain their original rational curves.
        // At w=1 this noncollinear conic is parabolic; above 1 it is hyperbolic.
    }

    #[test]
    fn profile_conic_ellipse_declines_nonfinite_controls_in_every_position() {
        let original = [
            Vec2::new(0.0, -2.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(0.0, 2.0),
        ];
        for position in 0..3 {
            for coordinate in 0..2 {
                for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    let mut controls = original;
                    if coordinate == 0 {
                        controls[position].x = invalid;
                    } else {
                        controls[position].y = invalid;
                    }
                    let [a, apex, b] = controls;
                    assert!(
                        conic_ellipse(a, apex, b, 0.6).is_none(),
                        "control={position}, coordinate={coordinate}, value={invalid}"
                    );
                }
            }
        }
    }

    #[test]
    fn profile_conic_ellipse_declines_degeneracy_and_unrepresentable_extraction() {
        for [a, apex, b] in [
            [Vec2::new(3.0, -2.0); 3],
            [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)],
            [
                Vec2::new(-3.0, -5.0),
                Vec2::new(1.0, 3.0),
                Vec2::new(5.0, 11.0),
            ],
            [
                Vec2::new(0.0, -2.0),
                Vec2::new(4.0, 0.0),
                Vec2::new(0.0, -2.0),
            ],
            [
                Vec2::new(0.0, -2.0),
                Vec2::new(0.0, -2.0),
                Vec2::new(0.0, 2.0),
            ],
            [
                Vec2::new(0.0, -2.0),
                Vec2::new(0.0, 2.0),
                Vec2::new(0.0, 2.0),
            ],
            // Finite inputs can still overflow the derived dot products or chord midpoint.
            [
                Vec2::new(f64::MAX, 0.0),
                Vec2::new(f64::MAX, f64::MAX),
                Vec2::new(0.0, f64::MAX),
            ],
            [
                Vec2::new(f64::MAX, 0.0),
                Vec2::new(f64::MAX, 1.0),
                Vec2::new(f64::MAX, 2.0),
            ],
        ] {
            assert!(
                conic_ellipse(a, apex, b, 0.6).is_none(),
                "{a:?}, {apex:?}, {b:?}"
            );
            assert!(conic_ellipse(b, apex, a, 0.6).is_none());
        }
        // A real but very small ellipse is also outside the inherited extraction guard;
        // this is a conservative recognition miss, not a claim of geometric degeneracy.
        assert!(conic_ellipse(
            Vec2::new(0.0, -2e-10),
            Vec2::new(4e-10, 0.0),
            Vec2::new(0.0, 2e-10),
            0.6
        )
        .is_none());
    }

    fn ellipse_unequal_pieces(three: bool) -> Loop2 {
        // Original integer controls on x²/225+y²/400=1. The three-piece neighboring
        // unit-ellipse dot products are -3/5, -7/25, -3/5; their weights are
        // sqrt((1+h)/2) = 1/sqrt(5), 3/5, 1/sqrt(5), rounded to nearest binary64.
        let pieces = if three {
            vec![
                (
                    Vec2::new(15.0, 0.0),
                    Vec2::new(15.0, 40.0),
                    Vec2::new(-9.0, 16.0),
                    0.4472135954999579,
                ),
                (
                    Vec2::new(-9.0, 16.0),
                    Vec2::new(-25.0, 0.0),
                    Vec2::new(-9.0, -16.0),
                    0.6,
                ),
                (
                    Vec2::new(-9.0, -16.0),
                    Vec2::new(15.0, -40.0),
                    Vec2::new(15.0, 0.0),
                    0.4472135954999579,
                ),
            ]
        } else {
            vec![
                (
                    Vec2::new(9.0, -16.0),
                    Vec2::new(25.0, 0.0),
                    Vec2::new(9.0, 16.0),
                    0.6,
                ),
                (
                    Vec2::new(9.0, 16.0),
                    Vec2::new(0.0, 25.0),
                    Vec2::new(-9.0, 16.0),
                    0.8,
                ),
                (
                    Vec2::new(-9.0, 16.0),
                    Vec2::new(-25.0, 0.0),
                    Vec2::new(-9.0, -16.0),
                    0.6,
                ),
                (
                    Vec2::new(-9.0, -16.0),
                    Vec2::new(0.0, -25.0),
                    Vec2::new(9.0, -16.0),
                    0.8,
                ),
            ]
        };
        Loop2 {
            segs: pieces
                .into_iter()
                .map(|(a, apex, b, w)| Seg2::Conic { a, apex, b, w })
                .collect(),
        }
    }

    fn transform_ellipse_pieces(
        path: &Loop2,
        center: Vec2,
        scale: f64,
        rotated: bool,
        reflected: bool,
    ) -> Loop2 {
        let transform = |p: Vec2| {
            let p = if rotated { Vec2::new(-p.y, p.x) } else { p };
            let p = if reflected { Vec2::new(p.x, -p.y) } else { p };
            center + p * scale
        };
        Loop2 {
            segs: path
                .segs
                .iter()
                .map(|seg| match *seg {
                    Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                        a: transform(a),
                        apex: transform(apex),
                        b: transform(b),
                        w,
                    },
                    _ => panic!("analytic fixture contains only conics"),
                })
                .collect(),
        }
    }

    #[test]
    fn profile_ellipse_recognizes_unequal_integer_partitions_through_the_real_candidate() {
        let far = 2.0_f64.powi(52);
        for three in [false, true] {
            let original = ellipse_unequal_pieces(three);
            for (center, scale) in [
                (Vec2::ZERO, 1.0),
                (Vec2::new(7.0, -5.0), 2.0),
                (Vec2::new(far, far), 1.0),
            ] {
                for rotated in [false, true] {
                    for reflected in [false, true] {
                        let transformed =
                            transform_ellipse_pieces(&original, center, scale, rotated, reflected);
                        let major = if rotated {
                            Vec2::new(20.0 * scale, 0.0)
                        } else {
                            Vec2::new(0.0, 20.0 * scale)
                        };
                        for mut path in [transformed.clone(), transformed.reversed()] {
                            for _ in 0..path.segs.len() {
                                assert_eq!(
                                    path.as_ellipse(),
                                    Some((center, major, 0.75)),
                                    "three={three}, {path:?}"
                                );
                                path.segs.rotate_left(1);
                            }
                        }
                    }
                }
            }
            // T(x,y)=(3x-4y,4x+3y) preserves integer controls. Its support is
            // [[8425,-2100],[-2100,7200]], with eigenvalues 10000 and 5625.
            // This exercises both the non-diagonal candidate and the support cross term.
            let transform = |p: Vec2| Vec2::new(3.0 * p.x - 4.0 * p.y, 4.0 * p.x + 3.0 * p.y);
            let inclined = Loop2 {
                segs: original
                    .segs
                    .iter()
                    .map(|seg| match *seg {
                        Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                            a: transform(a),
                            apex: transform(apex),
                            b: transform(b),
                            w,
                        },
                        _ => panic!("analytic fixture contains only conics"),
                    })
                    .collect(),
            };
            assert!(inclined.is_closed(0.0));
            for mut path in [inclined.clone(), inclined.reversed()] {
                for _ in 0..path.segs.len() {
                    assert_eq!(
                        path.as_ellipse(),
                        Some((Vec2::ZERO, Vec2::new(80.0, -60.0), 0.75)),
                        "three={three}, {path:?}"
                    );
                    path.segs.rotate_left(1);
                }
            }
        }
    }

    #[test]
    fn profile_ellipse_rejects_distinct_supports_even_with_shared_endpoints() {
        for three in [false, true] {
            let path = ellipse_unequal_pieces(three);
            let mut different_apex = path.clone();
            if let Seg2::Conic { apex, .. } = &mut different_apex.segs[0] {
                apex.x += 1e-8;
            }
            let mut different_weight = path.clone();
            if let Seg2::Conic { w, .. } = &mut different_weight.segs[0] {
                *w += 1e-8;
            }
            let mut different_vertex = path.clone();
            if let Seg2::Conic { b, .. } = &mut different_vertex.segs[0] {
                b.x += 1e-8;
            }
            if let Seg2::Conic { a, .. } = &mut different_vertex.segs[1] {
                a.x += 1e-8;
            }
            let mut different_half = path.clone();
            if let Seg2::Conic { apex, .. } = &mut different_half.segs[1] {
                apex.y += 0.25;
            }
            for changed in [
                different_apex,
                different_weight,
                different_vertex,
                different_half,
            ] {
                assert!(changed.is_closed(0.0));
                for mut ordered in [changed.clone(), changed.reversed()] {
                    for _ in 0..ordered.segs.len() {
                        assert_eq!(ordered.as_ellipse(), None, "{ordered:?}");
                        ordered.segs.rotate_left(1);
                    }
                }
            }
        }
    }

    #[test]
    fn profile_ellipse_rejects_gaps_retracing_and_repeated_unequal_turns() {
        let ellipse = ellipse_unequal_pieces(false);
        let q = ellipse.segs[0];
        let mut one_step_gap = ellipse.clone();
        if let Seg2::Conic { a, .. } = &mut one_step_gap.segs[1] {
            a.x = next_up(a.x);
        }
        let mut reordered = ellipse.clone();
        reordered.segs.swap(1, 2);
        for path in [
            one_step_gap,
            reordered,
            Loop2 {
                segs: vec![q, q.reversed(), q, q.reversed()],
            },
            Loop2 { segs: vec![q; 4] },
            Loop2 {
                segs: ellipse.segs.repeat(2),
            },
            Loop2 {
                segs: ellipse.segs[..3].to_vec(),
            },
            Loop2 {
                segs: vec![
                    q,
                    Seg2::Line {
                        a: Vec2::new(1.0, 0.0),
                        b: Vec2::ZERO,
                    },
                ],
            },
        ] {
            assert_eq!(path.as_ellipse(), None, "{path:?}");
            assert_eq!(path.reversed().as_ellipse(), None, "{path:?}");
        }
    }

    #[test]
    fn profile_ellipse_uses_local_cells_at_large_world_coordinates() {
        let c = 2.0_f64.powi(52);
        let original = transform_ellipse_pieces(
            &ellipse_unequal_pieces(false),
            Vec2::new(c, c),
            1.0,
            false,
            false,
        );
        assert!(
            original.as_ellipse().is_some(),
            "large coordinates alone are not a refusal"
        );
        let mut changed = original.clone();
        if let Seg2::Conic { b, .. } = &mut changed.segs[1] {
            b.x += 0.5;
        }
        if let Seg2::Conic { a, .. } = &mut changed.segs[2] {
            a.x += 0.5;
        }
        assert!(changed.is_closed(0.0));
        assert_eq!(
            changed.as_ellipse(),
            None,
            "a resolved local displacement cannot fit a world-coordinate allowance"
        );
        // The canonical left chord still has length32. Changing its apex from -25 to -24
        // proposes a center with a fractional offset that cannot survive addition to c.
        let mut center_loss = original.clone();
        if let Seg2::Conic { apex, .. } = &mut center_loss.segs[2] {
            apex.x += 1.0;
        }
        assert_eq!(
            center_loss.as_ellipse(),
            None,
            "final-center loss must fit the local cell"
        );
        let mut outlier = original;
        if let Seg2::Conic { apex, .. } = &mut outlier.segs[0] {
            apex.x = 1e100;
        }
        assert_eq!(
            outlier.as_ellipse(),
            None,
            "an apex cannot enlarge the vertex budget"
        );
    }

    #[test]
    fn profile_ellipse_declines_hostile_general_partitions() {
        let original = ellipse_unequal_pieces(false);
        for weight in [
            0.0,
            -1.0,
            1e-9,
            1.0 - 1e-6,
            1.0,
            1.1,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let mut path = original.clone();
            if let Seg2::Conic { w, .. } = &mut path.segs[0] {
                *w = weight;
            }
            assert_eq!(path.as_ellipse(), None, "w={weight}");
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for control in 0..3 {
                for coordinate in 0..2 {
                    let mut path = original.clone();
                    if let Seg2::Conic { a, apex, b, .. } = &mut path.segs[0] {
                        let point = match control {
                            0 => a,
                            1 => apex,
                            _ => b,
                        };
                        if coordinate == 0 {
                            point.x = value;
                        } else {
                            point.y = value;
                        }
                    }
                    assert_eq!(path.as_ellipse(), None);
                }
            }
        }
        for scale in [0.0, 1e-200, 1e200] {
            assert_eq!(
                transform_ellipse_pieces(&original, Vec2::ZERO, scale, false, false).as_ellipse(),
                None
            );
        }
        let mut collinear = original.clone();
        for seg in &mut collinear.segs {
            if let Seg2::Conic { a, apex, b, .. } = seg {
                a.y = 0.0;
                apex.y = 0.0;
                b.y = 0.0;
            }
        }
        assert_eq!(collinear.as_ellipse(), None);
        // Keep exact joins and nonzero chords. The left chord of length 32 supplies
        // the known support; the first piece has antipodal endpoints on that support.
        let antipodal = Loop2 {
            segs: vec![
                Seg2::Conic {
                    a: Vec2::new(15.0, 0.0),
                    apex: Vec2::new(0.0, 20.0),
                    b: Vec2::new(-15.0, 0.0),
                    w: 0.6,
                },
                Seg2::Conic {
                    a: Vec2::new(-15.0, 0.0),
                    apex: Vec2::new(-15.0, -10.0),
                    b: Vec2::new(-9.0, -16.0),
                    w: 0.8944271909999159,
                },
                Seg2::Conic {
                    a: Vec2::new(-9.0, -16.0),
                    apex: Vec2::new(-25.0, 0.0),
                    b: Vec2::new(-9.0, 16.0),
                    w: 0.6,
                },
                Seg2::Conic {
                    a: Vec2::new(-9.0, 16.0),
                    apex: Vec2::new(15.0, 40.0),
                    b: Vec2::new(15.0, 0.0),
                    w: 0.4472135954999579,
                },
            ],
        };
        assert!(antipodal.is_closed(0.0));
        assert!(antipodal
            .segs
            .iter()
            .all(|seg| matches!(*seg, Seg2::Conic { a, b, .. } if a != b)));
        assert_eq!(antipodal.as_ellipse(), None);
    }

    #[test]
    fn profile_ellipse_arithmetic_retains_exact_dyadic_residuals() {
        use crate::prism::recognition::EllipseScalar as Number;
        let number = |x| Number::exact(x).expect("finite dyadic fixture");
        let large = 2.0_f64.powi(53);
        let sum = number(large).plus(number(1.0)).expect("finite sum");
        assert_eq!((sum.hi, sum.lo, sum.radius), (large, 1.0, 0.0));
        let cancellation = sum.minus(number(large)).expect("exact cancellation");
        assert_eq!(
            (cancellation.hi, cancellation.lo, cancellation.radius),
            (1.0, 0.0, 0.0)
        );
        let e = 2.0_f64.powi(-27);
        let product = number(1.0 + e)
            .times(number(1.0 - e))
            .expect("finite product");
        // (1+2^-27)(1-2^-27) = 1-2^-54; the low term cannot be discarded.
        assert_eq!((product.hi, product.lo), (1.0, -e * e));
        assert!(product.radius < 2.0_f64.powi(-100));
        let third = 2.0_f64.powi(-108);
        let three_terms = number(1.0)
            .plus(number(2.0_f64.powi(-54)))
            .expect("pair")
            .plus(number(third))
            .expect("three terms");
        assert!(
            three_terms.radius >= third,
            "a discarded third component remains in the radius"
        );
    }

    #[test]
    fn profile_ellipse_arithmetic_certifies_quotients_and_roots() {
        use crate::prism::recognition::EllipseScalar as Number;
        let number = |x| Number::exact(x).expect("finite fixture");
        let dyadic = number(1.0)
            .plus(number(2.0_f64.powi(-53)))
            .expect("exact two-term numerator");
        let quotient = dyadic.divided_by(number(2.0)).expect("dyadic quotient");
        assert_eq!((quotient.hi, quotient.lo), (0.5, 2.0_f64.powi(-54)));
        assert!(quotient.radius < 2.0_f64.powi(-100));
        let third = number(1.0).divided_by(number(3.0)).expect("one third");
        let correction = third.minus(number(1.0 / 3.0)).expect("retain correction");
        // The exact correction is 1/(3*2^54), strictly between these independent dyadics.
        assert_eq!(
            correction
                .minus(number(2.0_f64.powi(-56)))
                .expect("lower comparison")
                .sign(),
            Some(1)
        );
        assert_eq!(
            number(2.0_f64.powi(-55))
                .minus(correction)
                .expect("upper comparison")
                .sign(),
            Some(1)
        );
        assert!(third.radius < 2.0_f64.powi(-100));
        let e = 2.0_f64.powi(-27);
        let square = number(1.0 + 2.0 * e)
            .plus(number(e * e))
            .expect("(1+e)^2 exactly");
        let root = square.square_root().expect("positive exact-dyadic square");
        assert!(root
            .minus(number(1.0 + e))
            .expect("compare analytic root")
            .fits(2.0_f64.powi(-100)));
    }

    #[test]
    fn profile_ellipse_arithmetic_refuses_uncertainty_and_preserves_subnormal_bounds() {
        use crate::prism::recognition::EllipseScalar as Number;
        let number = |x| Number::exact(x).expect("finite fixture");
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Number::exact(invalid).is_none());
        }
        assert!(number(f64::MAX).plus(number(f64::MAX)).is_none());
        assert!(number(f64::MAX).times(number(2.0)).is_none());
        assert!(number(1.0).divided_by(number(0.0)).is_none());
        assert!(number(-1.0).square_root().is_none());
        assert!(number(0.0).square_root().is_none());
        let minimum = f64::from_bits(1);
        let underflow = number(minimum)
            .times(number(0.5))
            .expect("half a minimum subnormal is enclosed");
        assert_eq!((underflow.hi, underflow.lo), (0.0, 0.0));
        assert!(underflow.radius >= minimum);
        assert!(!underflow.fits(0.0));
        assert_eq!(underflow.sign(), None);
        let radius = 2.0_f64.powi(-53);
        assert!(number(radius).fits(radius));
        assert!(!number(next_up(radius)).fits(radius));
        let uncertain = Number {
            hi: 0.0,
            lo: 0.0,
            radius: 2.0 * radius,
        };
        assert!(
            !uncertain.fits(radius),
            "intersection with the cell does not prove inclusion"
        );
        assert!(number(1.0).divided_by(uncertain).is_none());
    }

    #[test]
    fn profile_ellipse_weight_cells_are_asymmetric_and_local_extent_does_not_round_up() {
        use crate::prism::recognition::{ellipse_local_resolution, EllipseScalar as Number};
        let number = |x| Number::exact(x).expect("finite fixture");
        let center = number(0.5);
        let lower_midpoint = center
            .minus(number(2.0_f64.powi(-55)))
            .expect("lower midpoint");
        let upper_midpoint = center
            .plus(number(2.0_f64.powi(-54)))
            .expect("upper midpoint");
        assert_eq!(lower_midpoint.rounds_to(0.5), Some(false));
        assert_eq!(upper_midpoint.rounds_to(0.5), Some(false));
        assert_eq!(
            center
                .minus(number(2.0_f64.powi(-56)))
                .expect("inside lower cell")
                .rounds_to(0.5),
            Some(true)
        );
        assert_eq!(
            center
                .plus(number(2.0_f64.powi(-55)))
                .expect("inside upper cell")
                .rounds_to(0.5),
            Some(true)
        );
        assert_eq!(
            center
                .minus(number(3.0 * 2.0_f64.powi(-56)))
                .expect("outside lower cell")
                .rounds_to(0.5),
            Some(false)
        );
        let local = 2.0_f64.powi(-52);
        let c = 2.0_f64.powi(52);
        assert_eq!(ellipse_local_resolution(-4.0, 4.0), Some(local));
        assert_eq!(ellipse_local_resolution(c - 4.0, c + 4.0), Some(local));
        assert_eq!(
            ellipse_local_resolution(-4.0, next_down(4.0_f64)),
            Some(local),
            "rounded extent must not select a larger binade"
        );
        assert!(ellipse_local_resolution(0.0, f64::from_bits(1)).is_none());
        assert!(ellipse_local_resolution(-f64::MAX, f64::MAX).is_none());
        assert!(ellipse_local_resolution(1.0, 1.0).is_none());
    }
}

mod arc_conversion_regressions {
    use super::super::arc_conversion::*;
    use super::super::profile::Seg2;
    use crate::math::Vec2;

    #[test]
    fn admitted_tiny_arc_preserves_the_source_but_refuses_collapsed_wire_angles() {
        let s = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 8.0,
            start: 0.0,
            sweep: 1e-16,
        };
        assert!(admit_piece(&s, 0, 3).is_ok());
        // An explicit coarse scalar encoding makes this a geometry refusal test,
        // independent of any file format's decimal spelling.
        let encoding = ScalarEncoding {
            length: |v| Some(if v.abs() < 1e-12 { 0.0 } else { v }),
            dimensionless: |v| Some(if v.abs() < 1e-12 { 0.0 } else { v }),
            min_ellipse_ratio: 1e-12,
            turn_snap_radians: 1e-12,
        };
        let err = exact_piece(&s, 0, 3, encoding).unwrap_err();
        assert_eq!(err.piece, Some((0, 3)));
        assert_eq!(
            err.reason,
            "arc endpoints collapse in the angular representation"
        );
    }

    #[test]
    fn arc_degree_conversion_refuses_large_phase_error_for_both_orientations() {
        for start in [2.0_f64.powi(50), -2.0_f64.powi(50)] {
            for sweep in [1.0, -1.0] {
                let arc = Seg2::Arc {
                    center: Vec2::ZERO,
                    radius: 8.0,
                    start,
                    sweep,
                };
                assert!(admit_piece(&arc, 2, 5).is_ok());
                let error = exact_piece(&arc, 2, 5, scalar_encoding(Some)).unwrap_err();
                assert_eq!(error.piece, Some((2, 5)));
                assert_eq!(
                    error.reason,
                    "arc degree endpoints exceed the angular rounding budget"
                );
            }
        }
    }

    #[test]
    fn arc_degree_certificate_includes_the_source_addition_residual() {
        // The rounded end is within B of its emitted angle, but the exact
        // start+sweep is about 2.6944e-13 degrees away, beyond B. Certifying
        // the rounded end and only the span within 2B would accept this arc.
        let arc = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 8.0,
            start: -125.5,
            sweep: -5.09,
        };
        let error = exact_piece(&arc, 0, 1, scalar_encoding(Some)).unwrap_err();
        assert_eq!(
            error.reason,
            "arc degree endpoints exceed the angular rounding budget"
        );
    }

    #[test]
    fn certified_arc_degrees_keep_quadrants_wraparound_and_small_arcs() {
        use std::f64::consts::{FRAC_PI_2, PI};
        for (start, sweep, expected) in [
            (0.0, FRAC_PI_2, (0.0, 90.0)),
            (PI, -FRAC_PI_2, (90.0, 180.0)),
            (-FRAC_PI_2, FRAC_PI_2, (270.0, 0.0)),
        ] {
            let arc = Seg2::Arc {
                center: Vec2::ZERO,
                radius: 8.0,
                start,
                sweep,
            };
            let shape = exact_piece(&arc, 0, 1, scalar_encoding(Some)).unwrap();
            assert!(matches!(shape, PathShape::Arc { a0, a1, .. } if (a0, a1) == expected));
        }
        for start in [0.0, -1e-13, 1e-13, 1.0_f64.atan2(2.0)] {
            let arc = Seg2::Arc {
                center: Vec2::ZERO,
                radius: 8.0,
                start,
                sweep: 1.0,
            };
            assert!(exact_piece(&arc, 0, 1, scalar_encoding(Some)).is_ok());
        }
        for sweep in [1e-13, 1e-16] {
            let arc = Seg2::Arc {
                center: Vec2::ZERO,
                radius: 8.0,
                start: 0.0,
                sweep,
            };
            assert!(
                matches!(exact_piece(&arc, 0, 1, scalar_encoding(Some)).unwrap(),
                PathShape::Arc { a0: 0.0, a1, .. } if a1 > 0.0)
            );
        }
    }

    fn scalar_encoding(round_trip: fn(f64) -> Option<f64>) -> ScalarEncoding {
        ScalarEncoding {
            length: round_trip,
            dimensionless: round_trip,
            min_ellipse_ratio: 1e-12,
            turn_snap_radians: 1e-12,
        }
    }

    #[test]
    fn refused_line_endpoint_never_certifies_a_path_or_a_degenerate_line() {
        let encoding = scalar_encoding(|v| (v <= 10.0).then_some(v));
        for (a, b) in [
            (Vec2::ZERO, Vec2::new(11.0, 0.0)),
            (Vec2::new(11.0, 0.0), Vec2::ZERO),
            (Vec2::new(11.0, 0.0), Vec2::new(11.0, 0.0)),
        ] {
            let segs = [
                Seg2::Line {
                    a: Vec2::ZERO,
                    b: Vec2::new(1.0, 0.0),
                },
                Seg2::Line { a, b },
            ];
            let error =
                path_shapes(PlanarPath::Open(&segs), PathStyle::Exact, encoding).unwrap_err();
            assert_eq!(error.piece, Some((1, 2)));
            assert_eq!(error.reason, "line endpoint cannot be encoded");
        }
    }

    #[test]
    fn polylines_and_centres_are_admitted_only_as_the_encoding_writes_them() {
        // A caller whose lengths stop at 10, on a 1e-3 grid, with three decimals elsewhere.
        fn grid(v: f64) -> Option<f64> {
            (v.abs() <= 10.0).then_some((v * 1e3).round() / 1e3)
        }
        let encoding = ScalarEncoding {
            length: grid,
            dimensionless: |v| Some((v * 1e3).round() / 1e3),
            min_ellipse_ratio: 1e-12,
            turn_snap_radians: 1e-12,
        };
        let far = Seg2::Line {
            a: Vec2::ZERO,
            b: Vec2::new(11.0, 0.0),
        };
        let error =
            path_shapes(PlanarPath::Open(&[far]), PathStyle::Polyline, encoding).unwrap_err();
        assert_eq!(error.reason, "line endpoint cannot be encoded");
        let off_grid = Seg2::Arc {
            center: Vec2::new(11.0, 0.0),
            radius: 2.0,
            start: 0.0,
            sweep: 0.5,
        };
        for style in [PathStyle::Exact, PathStyle::Polyline] {
            let error = path_shapes(PlanarPath::Open(&[off_grid]), style, encoding).unwrap_err();
            assert_eq!(error.reason, "arc centre cannot be encoded");
        }
        // A bulge of 0.00025 is written as 0, and two ends 0.0002 apart as one point: neither arc
        // is packed into a polyline, and each is written exactly instead.
        let flat = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 1.0,
            start: 0.0,
            sweep: 0.001,
        };
        let short = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 0.0002,
            start: 0.0,
            sweep: 1.0,
        };
        let packed = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 1.0,
            start: 0.0,
            sweep: 1.0,
        };
        assert!(matches!(
            path_shapes(PlanarPath::Open(&[flat]), PathStyle::Polyline, encoding).unwrap()[..],
            [PathShape::Arc { .. }]
        ));
        assert!(matches!(
            path_shapes(PlanarPath::Open(&[packed]), PathStyle::Polyline, encoding).unwrap()[..],
            [PathShape::BulgedPolyline { .. }]
        ));
        assert!(
            path_shapes(PlanarPath::Open(&[short]), PathStyle::Polyline, encoding)
                .map_or(true, |shapes| matches!(shapes[..], [PathShape::Arc { .. }]))
        );
    }

    #[test]
    fn refused_arc_angle_never_certifies_a_one_sided_encoding() {
        let arc = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 2.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        };
        // Angles are dimensionless: only that callback refuses them.
        for dimensionless in [
            (|v| (v != 90.0).then_some(v)) as fn(f64) -> Option<f64>,
            |v| (v != 0.0).then_some(v),
        ] {
            let encoding = ScalarEncoding {
                dimensionless,
                ..scalar_encoding(Some)
            };
            let error = exact_piece(&arc, 2, 4, encoding).unwrap_err();
            assert_eq!(error.piece, Some((2, 4)));
            assert_eq!(error.reason, "arc angle cannot be encoded");
        }
    }

    #[test]
    fn refused_ellipse_parameters_keep_the_exact_rational_controls() {
        let a = Vec2::new(2.0, 0.0);
        let apex = Vec2::new(2.0, 1.0);
        let b = Vec2::new(0.0, 1.0);
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let conic = Seg2::Conic { a, apex, b, w };
        assert!(matches!(
            exact_piece(&conic, 0, 1, scalar_encoding(Some)).unwrap(),
            PathShape::Ellipse { .. }
        ));
        // The quarter ellipse's pi/2 parameter is refused; all its rational
        // controls, weights and knots are representable in this same encoding.
        let encoding = scalar_encoding(|v| (!(1.0..2.0).contains(&v) || v == 1.0).then_some(v));
        let PathShape::Spline {
            degree,
            knots,
            ctrl,
            weights,
        } = exact_piece(&conic, 0, 1, encoding).unwrap()
        else {
            panic!("unencodable ellipse parameter must keep the rational source");
        };
        assert_eq!(degree, 2);
        assert_eq!(knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert_eq!(ctrl, [a, apex, b]);
        assert_eq!(weights, Some(vec![1.0, w, 1.0]));
        let error = exact_piece(
            &conic,
            0,
            1,
            scalar_encoding(|v| (!(1.9..2.1).contains(&v.abs())).then_some(v)),
        )
        .unwrap_err();
        assert_eq!(error.reason, "spline control cannot be encoded");
    }

    #[test]
    fn translated_conic_keeps_its_rational_definition_in_public_arc_conversion() {
        let c = 2.0_f64.powi(52);
        let w = 0.5_f64.sqrt();
        let a = Vec2::new(c + 4.0, 0.0);
        let apex = Vec2::new(c + 4.0, 2.0);
        let b = Vec2::new(c + 1.0, 2.0);
        for (a, apex, b) in [
            (a, apex, b),
            (
                Vec2::new(a.y, a.x),
                Vec2::new(apex.y, apex.x),
                Vec2::new(b.y, b.x),
            ),
        ] {
            for (start, end) in [(a, b), (b, a)] {
                let segs = [
                    Seg2::Line {
                        a: Vec2::ZERO,
                        b: start,
                    },
                    Seg2::Conic {
                        a: start,
                        apex,
                        b: end,
                        w,
                    },
                ];
                let shapes = path_shapes(
                    PlanarPath::Open(&segs),
                    PathStyle::Exact,
                    scalar_encoding(Some),
                )
                .expect("finite conic must retain its rational definition");
                assert_eq!(
                    shapes,
                    vec![
                        PathShape::Line(Vec2::ZERO, start),
                        PathShape::Spline {
                            degree: 2,
                            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                            ctrl: vec![start, apex, end],
                            weights: Some(vec![1.0, w, 1.0]),
                        },
                    ],
                    "refusing an unrepresentable ellipse must not lose or move source geometry"
                );
            }
        }
    }

    #[test]
    fn refused_spline_control_never_passes_a_partial_collapse_check() {
        let cubic = Seg2::Cubic {
            p0: Vec2::ZERO,
            p1: Vec2::new(1.0, 1.0),
            p2: Vec2::new(11.0, 1.0),
            p3: Vec2::new(2.0, 0.0),
        };
        let error =
            exact_piece(&cubic, 1, 3, scalar_encoding(|v| (v <= 10.0).then_some(v))).unwrap_err();
        assert_eq!(error.piece, Some((1, 3)));
        assert_eq!(error.reason, "spline control cannot be encoded");
    }

    #[test]
    fn nonfinite_decoded_scalars_never_certify_points_angles_or_positive_values() {
        for decoded in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let round_trip: fn(f64) -> Option<f64> = if decoded.is_nan() {
                |v| Some(if v == 2.0 { f64::NAN } else { v })
            } else if decoded.is_sign_positive() {
                |v| Some(if v == 2.0 { f64::INFINITY } else { v })
            } else {
                |v| Some(if v == 2.0 { f64::NEG_INFINITY } else { v })
            };
            let encoding = scalar_encoding(round_trip);
            let line = Seg2::Line {
                a: Vec2::ZERO,
                b: Vec2::new(2.0, 0.0),
            };
            assert_eq!(
                exact_piece(&line, 0, 1, encoding).unwrap_err().reason,
                "line endpoint cannot be encoded"
            );
            let circle = Seg2::Arc {
                center: Vec2::ZERO,
                radius: 2.0,
                start: 0.0,
                sweep: std::f64::consts::TAU,
            };
            assert_eq!(
                exact_piece(&circle, 0, 1, encoding).unwrap_err().reason,
                "arc radius is not positive at writer precision"
            );
        }
        let arc = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 2.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        };
        let error = exact_piece(
            &arc,
            0,
            1,
            scalar_encoding(|v| Some(if v == 90.0 { f64::INFINITY } else { v })),
        )
        .unwrap_err();
        assert_eq!(error.reason, "arc angle cannot be encoded");
    }

    #[test]
    fn bulge_bounds_include_the_directed_cardinal_extremum() {
        let pts = bulged_edge_points(
            Vertex {
                at: Vec2::new(1.0, 0.0),
                bulge: 1.0,
            },
            Vec2::new(-1.0, 0.0),
        )
        .unwrap();
        assert!(pts.contains(&Vec2::new(0.0, 1.0)));
        assert!(!pts.contains(&Vec2::new(0.0, -1.0)));
        assert_eq!(pts.len(), 5);
    }
}

#[test]
fn symmetric_offset_intervals_and_probes_are_symmetric_about_sketch_plane() {
    assert_eq!(
        symmetric_offset_intervals(3.0, 5.0),
        [(3.0, 8.0), (-8.0, -3.0)]
    );
    assert_eq!(
        symmetric_offset_intervals(-3.0, 5.0),
        [(3.0, 8.0), (-8.0, -3.0)]
    );

    // `base` in auto-operation is already shifted by `start` along `n`.
    let n = [0.0, 0.0, 1.0];
    let pos_start_probes = symmetric_offset_probe_points([0.0, 0.0, 3.0], n, 3.0, 10.0, 0.5);
    assert_eq!(
        pos_start_probes,
        [
            [0.0, 0.0, 3.5],
            [0.0, 0.0, 5.5],
            [0.0, 0.0, -3.5],
            [0.0, 0.0, -5.5],
        ]
    );
    let neg_start_probes = symmetric_offset_probe_points([0.0, 0.0, -3.0], n, -3.0, 10.0, 0.5);
    assert_eq!(neg_start_probes, pos_start_probes);
}

/// Straight members (`member`): closed forms for the middle line, the extrusion range, trims to
/// planes and butt joints, and the refusals.
mod member {
    use std::f64::consts::FRAC_1_SQRT_2;

    use crate::eval::Frame;
    use crate::geom::vector3d::{Point3, Vec3};
    use crate::math::Vec2;
    use crate::prism::member::{axis, butt, extent, to_plane, End, Member, Refusal, Side};

    fn frame(o: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Frame {
        Frame {
            origin: o,
            x_axis: x,
            y_axis: y,
        }
    }

    /// The plane through `o` with unit normal `n`: its x the unit vector square to `n` in the
    /// plane of `n` and world y, or world x when `n` is along y; y = n × x.
    fn plane(o: [f64; 3], n: [f64; 3]) -> Frame {
        let n = Vec3::new(n[0], n[1], n[2]);
        let up = if n.y.abs() > 0.9 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let x = (up - n * up.dot(n)).unitize().unwrap();
        let y = n.cross(x);
        frame(o, [x.x, x.y, x.z], [y.x, y.y, y.z])
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A `w` × `w` bar centred on the line from `a` to `b` along +x, its section in y and z.
    fn bar_along_x(a: f64, b: f64, w: f64) -> Member {
        let h = w / 2.0;
        Member::along(
            frame([a, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            Point3::new(a, 0.0, 0.0),
            Point3::new(b, 0.0, 0.0),
            Vec2::new(-h, -h),
            Vec2::new(h, h),
        )
        .unwrap()
    }

    #[test]
    fn a_line_without_length_or_a_section_without_area_is_degenerate() {
        let f = frame([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let p = Point3::new(0.0, 0.0, 0.0);
        let r = Member::along(f.clone(), p, p, Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0));
        assert_eq!(r.unwrap_err(), Refusal::Degenerate);
        let r = Member::along(
            f,
            p,
            Point3::new(1.0, 0.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, -1.0),
        );
        assert_eq!(r.unwrap_err(), Refusal::Degenerate);
    }

    /// The plane x + z = 150 cuts a 10 × 10 bar along x: its middle line at 150, its corners at
    /// 145 and 155, so the section runs past 155 by the slant and 1 more before the cut.
    #[test]
    fn a_slanted_end_crosses_the_middle_line_where_the_plane_does() {
        let m = bar_along_x(0.0, 100.0, 10.0);
        let ends = [
            None,
            Some(plane(
                [150.0, 0.0, 0.0],
                [FRAC_1_SQRT_2, 0.0, FRAC_1_SQRT_2],
            )),
        ];
        let a = axis(&m, &ends).unwrap();
        assert!(
            near(a.t0, 0.0) && near(a.t1, 150.0) && near(a.length, 150.0),
            "{a:?}"
        );
        let e = extent(&m, &ends).unwrap();
        assert!(
            near(e.from, 0.0) && near(e.to, 155.0 + 10.0 + 1.0) && e.cut == [false, true],
            "{e:?}"
        );
        // Square ends: the line's own ends, nothing cut.
        let e = extent(&m, &[None, None]).unwrap();
        assert!(
            near(e.from, 0.0) && near(e.to, 100.0) && e.cut == [false, false],
            "{e:?}"
        );
    }

    /// Ends x + y = 0 and x + y = 30 are parallel: every line through the 45 × 45 section keeps
    /// 30 between them, though their extents along the line overlap. Ends x + y = 0 and
    /// x − y = 30 cross inside the section.
    #[test]
    fn parallel_slanted_ends_leave_a_short_member_and_crossing_ends_are_refused() {
        let m = bar_along_x(0.0, 30.0, 45.0);
        let start = Some(plane([0.0; 3], [-FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0]));
        let parallel = [
            start.clone(),
            Some(plane([30.0, 0.0, 0.0], [FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0])),
        ];
        let e = extent(&m, &parallel).unwrap();
        assert!(e.cut == [true, true], "{e:?}");
        assert!(near(axis(&m, &parallel).unwrap().length, 30.0));
        let crossing = [
            start,
            Some(plane(
                [30.0, 0.0, 0.0],
                [FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0],
            )),
        ];
        assert_eq!(extent(&m, &crossing).unwrap_err(), Refusal::EndsCross);
    }

    #[test]
    fn end_planes_facing_into_the_member_running_along_it_or_too_far_are_refused() {
        let m = bar_along_x(0.0, 100.0, 10.0);
        let into = [None, Some(plane([90.0, 0.0, 0.0], [-1.0, 0.0, 0.0]))];
        assert_eq!(extent(&m, &into).unwrap_err(), Refusal::WrongWay);
        let along = [None, Some(plane([0.0, 50.0, 0.0], [0.0, 1.0, 0.0]))];
        assert_eq!(extent(&m, &along).unwrap_err(), Refusal::WrongWay);
        assert_eq!(axis(&m, &along).unwrap_err(), Refusal::RunsAlong);
        let far = [None, Some(plane([2e6, 0.0, 0.0], [1.0, 0.0, 0.0]))];
        assert_eq!(extent(&m, &far).unwrap_err(), Refusal::TooFar);
    }

    /// A bar from 0 to 100 trimmed to x = 150 with a gap of 2: its end moves, to the plane
    /// x = 148 facing on along the line. The start, asked for, moves there instead.
    #[test]
    fn a_trim_to_a_plane_moves_the_nearer_end_short_of_it_by_the_gap() {
        let m = bar_along_x(0.0, 100.0, 10.0);
        let a = axis(&m, &[None, None]).unwrap();
        let target = plane([150.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
        let (which, cut) = to_plane(&m, &a, End::Nearer, &target, 2.0).unwrap();
        assert_eq!(which, 1);
        assert!(
            near(cut.origin.x, 148.0) && near(cut.normal.dot(Vec3::new(1.0, 0.0, 0.0)), 1.0),
            "{cut:?}"
        );
        let (which, cut) = to_plane(&m, &a, End::Start, &target, 0.0).unwrap();
        assert_eq!(which, 0);
        assert!(
            near(cut.origin.x, 150.0) && near(cut.normal.dot(Vec3::new(-1.0, 0.0, 0.0)), 1.0),
            "{cut:?}"
        );
        // A plane along the bar doesn't trim it.
        assert!(to_plane(
            &m,
            &a,
            End::Nearer,
            &plane([0.0, 30.0, 0.0], [0.0, 1.0, 0.0]),
            0.0
        )
        .is_none());
    }

    /// A 45 × 45 post along +y at x = 0 (its section's x is world −x, y up) and a rail along +x
    /// from 100: the rail's start butts against the post's side facing it, x = 22.5, which is
    /// the post's left (−x of its section). Its right faces away; a parallel rail misses.
    #[test]
    fn a_butt_joint_cuts_the_nearer_end_on_the_targets_side_facing_it() {
        let post = Member::along(
            frame([0.0, -100.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            Point3::new(0.0, -100.0, 0.0),
            Point3::new(0.0, 100.0, 0.0),
            Vec2::new(-22.5, -22.5),
            Vec2::new(22.5, 22.5),
        )
        .unwrap();
        let post_axis = axis(&post, &[None, None]).unwrap();
        let rail = bar_along_x(100.0, 300.0, 45.0);
        let rail_axis = axis(&rail, &[None, None]).unwrap();
        let (which, cut) = butt(&rail, &rail_axis, End::Nearer, &post, &post_axis, None, 0.0)
            .unwrap()
            .unwrap();
        assert_eq!(which, 0);
        assert!(
            near(cut.origin.x, 22.5) && near(cut.normal.dot(Vec3::new(-1.0, 0.0, 0.0)), 1.0),
            "{cut:?}"
        );
        let left = butt(
            &rail,
            &rail_axis,
            End::Nearer,
            &post,
            &post_axis,
            Some(Side::Left),
            0.0,
        )
        .unwrap()
        .unwrap();
        assert_eq!(left, (which, cut));
        let away = butt(
            &rail,
            &rail_axis,
            End::Nearer,
            &post,
            &post_axis,
            Some(Side::Right),
            0.0,
        );
        assert_eq!(away.unwrap_err(), Refusal::FacesAway);
        let beside = Member::along(
            frame([100.0, 200.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            Point3::new(100.0, 200.0, 0.0),
            Point3::new(300.0, 200.0, 0.0),
            Vec2::new(-22.5, -22.5),
            Vec2::new(22.5, 22.5),
        )
        .unwrap();
        let beside_axis = axis(&beside, &[None, None]).unwrap();
        assert!(butt(
            &beside,
            &beside_axis,
            End::Nearer,
            &post,
            &post_axis,
            None,
            0.0
        )
        .unwrap()
        .is_none());
        assert!(butt(
            &rail,
            &rail_axis,
            End::Nearer,
            &beside,
            &beside_axis,
            None,
            0.0
        )
        .unwrap()
        .is_none());
    }
}

#[test]
/// Curved face boundaries contribute their actual area and projected extent to contact checks.
fn gusset_face_boundaries_are_measured_on_their_exact_curves() {
    use super::gusset::{
        conic_share, counter_clockwise, loop_signed_area, scalar_extent, seg_extent,
    };
    use super::profile::{Loop2, Seg2};
    use crate::math::Vec2 as V;
    use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};
    // A clockwise circle is turned counter-clockwise and encloses exactly pi.
    let circle = Loop2 {
        segs: vec![Seg2::Arc {
            center: V::new(2., 0.),
            radius: 1.,
            start: 0.,
            sweep: -TAU,
        }],
    };
    assert!((loop_signed_area(&circle).unwrap() + PI).abs() < 1e-12);
    assert!((loop_signed_area(&counter_clockwise(circle).unwrap()).unwrap() - PI).abs() < 1e-12);
    // A quarter arc reaches its extreme between its ends, not at them.
    let quarter = Seg2::Arc {
        center: V::new(0., 0.),
        radius: 1.,
        start: -0.25 * PI,
        sweep: 0.5 * PI,
    };
    let (lo, hi) = seg_extent(&quarter, V::new(1., 0.)).unwrap();
    assert!(
        (hi - 1.).abs() < 1e-12 && (lo - FRAC_1_SQRT_2).abs() < 1e-12,
        "{lo} {hi}"
    );
    // A cubic's bulge counts: (0,0)-(0,1)-(1,1)-(1,0) peaks at y = 3/4 and encloses 3/5 with its chord.
    let cubic = Seg2::Cubic {
        p0: V::new(0., 0.),
        p1: V::new(0., 1.),
        p2: V::new(1., 1.),
        p3: V::new(1., 0.),
    };
    let (_, top) = seg_extent(&cubic, V::new(0., 1.)).unwrap();
    assert!((top - 0.75).abs() < 1e-9, "{top}");
    let closed = Loop2 {
        segs: vec![
            cubic,
            Seg2::Line {
                a: V::new(1., 0.),
                b: V::new(0., 0.),
            },
        ],
    };
    assert!((loop_signed_area(&closed).unwrap().abs() - 0.6).abs() < 1e-12);
    // An extent that can't be resolved in the halvings allowed is an error, never the hull.
    assert!(scalar_extent(vec![(0., 1.), (1., 1.), (1., 1.), (0., 1.)], 0).is_err());

    // Conics: a quarter circle as a rational quadratic (w = cos 45°) closed through the centre
    // encloses pi/4 exactly, and reaches 1 along the diagonal.
    let arc = Seg2::Conic {
        a: V::new(1., 0.),
        apex: V::new(1., 1.),
        b: V::new(0., 1.),
        w: FRAC_1_SQRT_2,
    };
    let sector = Loop2 {
        segs: vec![
            arc,
            Seg2::Line {
                a: V::new(0., 1.),
                b: V::new(0., 0.),
            },
            Seg2::Line {
                a: V::new(0., 0.),
                b: V::new(1., 0.),
            },
        ],
    };
    assert!((loop_signed_area(&sector).unwrap() - PI / 4.).abs() < 1e-12);
    let (_, far) = seg_extent(&arc, V::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2)).unwrap();
    assert!((far - 1.).abs() < 1e-9, "{far}");
    // A parabola (w = 1) takes 2/3 of its control triangle; a hyperbola (w = cosh 1) takes
    // cosh 1 (sinh 1 cosh 1 - 1) / sinh³ 1; across w = 1 the share is continuous.
    assert!((conic_share(1.).unwrap() - 2. / 3.).abs() < 1e-15);
    let (c, sh) = (1f64.cosh(), 1f64.sinh());
    assert!((conic_share(c).unwrap() - c * (sh * c - 1.) / (sh * sh * sh)).abs() < 1e-12);
    for w in [1. - 1.1e-4, 1. - 0.9e-4, 1. + 0.9e-4, 1. + 1.1e-4] {
        assert!(
            (conic_share(w).unwrap() - (2. / 3. - 4. / 15. * (1. - w))).abs() < 1e-8,
            "{w}"
        );
    }
    let parabola = Seg2::Conic {
        a: V::new(-1., 0.),
        apex: V::new(0., 2.),
        b: V::new(1., 0.),
        w: 1.,
    };
    let cap = Loop2 {
        segs: vec![
            parabola,
            Seg2::Line {
                a: V::new(1., 0.),
                b: V::new(-1., 0.),
            },
        ],
    };
    assert!((loop_signed_area(&cap).unwrap().abs() - 4. / 3.).abs() < 1e-12);
    // A weight that isn't positive is refused, not measured.
    for w in [0., -0.5, f64::NAN] {
        let bad = Seg2::Conic {
            a: V::new(1., 0.),
            apex: V::new(1., 1.),
            b: V::new(0., 1.),
            w,
        };
        assert!(
            seg_extent(&bad, V::new(1., 0.)).is_err() && conic_share(w).is_err(),
            "{w}"
        );
    }
}

#[test]
fn gusset_plan_keeps_the_joint_offset_contacts_and_fabrication_cuts() {
    use super::gusset::{self, Dimensions, Face, Placement};
    use super::profile::Region2;
    use crate::eval::Frame;
    use crate::math::Transform;

    let rectangle = |x0, x1, y0, y1| Region2 {
        outer: polygon(&[
            Vec2::new(x0, y0),
            Vec2::new(x1, y0),
            Vec2::new(x1, y1),
            Vec2::new(x0, y1),
        ]),
        holes: Vec::new(),
    };
    let support = |plane: Frame, region| Face {
        source: plane.clone(),
        plane,
        regions: vec![region],
        to_source: Transform::IDENTITY,
        to_author: Transform::IDENTITY,
        tolerance: 1e-7,
    };
    // x = 0 faces +X; z = 0 faces +Z. Their joint runs along -Y.
    let a = support(
        Frame {
            origin: [0.0; 3],
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        },
        rectangle(-20.0, 20.0, 0.0, 200.0),
    );
    let b = support(
        Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        },
        rectangle(0.0, 200.0, -20.0, 20.0),
    );
    let dimensions = Dimensions {
        leg1: 100.0,
        leg2: 100.0,
        thickness: 6.0,
        relief: 0.0,
        clip: 0.0,
        offset: 5.0,
    };
    let plan = gusset::plan([&a, &b], dimensions, Placement::Center).unwrap();
    assert_eq!(plan.plane.origin, [0.0, -5.0, 0.0]);
    assert_eq!((plan.start, plan.end), (-3.0, 3.0));
    // A right triangle of legs 100 has area 5000; each contact is 100 x 6.
    assert!((plan.profile.signed_area().unwrap().value - 5000.0).abs() < 1e-9);
    for contact in &plan.contacts {
        assert!((contact.region.signed_area().unwrap().value - 600.0).abs() < 1e-9);
    }
    let cut = gusset::plan(
        [&a, &b],
        Dimensions {
            relief: 10.0,
            clip: 10.0,
            ..dimensions
        },
        Placement::Positive,
    )
    .unwrap();
    assert_eq!((cut.start, cut.end), (0.0, 6.0));
    // Relief removes 10*10/2. Each tip removes 10*10/(2*sqrt(2)).
    let expected_area = 5000.0 - 50.0 - 100.0 / 2.0_f64.sqrt();
    assert!((cut.profile.signed_area().unwrap().value - expected_area).abs() < 1e-9);
    for contact in &cut.contacts {
        assert!((contact.region.signed_area().unwrap().value - 480.0).abs() < 1e-9);
    }
    let negative = gusset::plan([&a, &b], dimensions, Placement::Negative).unwrap();
    assert_eq!((negative.start, negative.end), (-6.0, 0.0));
}
