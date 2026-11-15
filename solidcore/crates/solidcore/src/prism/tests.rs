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

use super::extrude::extrude_region;
use super::measure::{equivalent, section};
use super::placement::LoopPlacement;
use super::profile::rational_curve;
use super::profile::{
    check_regions, curve_on_frame, largest_arc_sweep, smallest_arc_sweep, tapered_flange_ends,
    Loop2, ProfileRefusal, Region2, Seg2,
};
use super::read::{planar_curves, planar_pieces, PlanarPiece, Run};
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
