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
use super::profile::rational_curve;
use super::profile::FigurePlacement;
use super::profile::{box_fit_in_convex_polygon, place_figure_2d, regular_polygon_2d, BoxFit};
use super::profile::{check_regions, curve_on_frame, Loop2, ProfileRefusal, Region2, Seg2};
use super::profile::{MAX_BEZIER_CTRL, MAX_PROFILE_COORD, MAX_SEGMENTS};
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

#[test]
fn hostile_profiles_are_refused_by_kind() {
    let plate = rectangle(p(0.0, 0.0), p(10.0, 10.0));
    let mut bad = plate.clone();
    bad.outer.segs[0] = Seg2::Line {
        a: p(f64::NAN, 0.0),
        b: p(1.0, 0.0),
    };
    assert_eq!(
        check_regions([&bad].into_iter(), 1e-7),
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
        check_regions([&conic].into_iter(), 1e-7),
        Err(ProfileRefusal::FreeForm)
    );
    let open = Region2 {
        outer: Loop2 {
            segs: plate.outer.segs[..3].to_vec(),
        },
        holes: vec![],
    };
    assert_eq!(
        check_regions([&open].into_iter(), 1e-7),
        Err(ProfileRefusal::Budget)
    );
    assert_eq!(check_regions([&plate].into_iter(), 1e-7), Ok(()));
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

// Sheet figures: the closed forms of a placed figure, a regular outline's corners and the label
// box that fits inside an outline, and their hostile inputs refused.

fn close(a: Point2, b: Point2) -> bool {
    a.distance(b) <= 1e-12 * (1.0 + b.length())
}

/// A placement with no turn.
fn sized(scale: f64, origin: Point2) -> FigurePlacement {
    FigurePlacement {
        scale,
        angle: 0.0,
        origin,
    }
}

#[test]
fn sheet_figures_place_lines_arcs_and_curves_at_their_size() {
    let (s, at) = (3.5, p(100.0, 50.0));
    let mut ctrl = [p(0.0, 0.0); MAX_BEZIER_CTRL];
    ctrl[0] = p(0.0, 1.0);
    ctrl[1] = p(1.0, 2.0);
    ctrl[2] = p(2.0, 1.0);
    ctrl[3] = p(9.0, 9.0); // past the degree: unused, so left as it is
    let figure = [
        Seg2::Line {
            a: p(0.0, 0.0),
            b: p(1.0, 0.0),
        },
        Seg2::Arc {
            center: p(0.5, 0.5),
            radius: 0.5,
            start: 0.0,
            sweep: PI,
        },
        Seg2::Cubic {
            p0: p(0.0, 0.0),
            p1: p(0.0, 1.0),
            p2: p(1.0, 1.0),
            p3: p(1.0, 0.0),
        },
        Seg2::Conic {
            a: p(1.0, 0.0),
            apex: p(1.0, 1.0),
            b: p(0.0, 1.0),
            w: FRAC_1_SQRT_2,
        },
        Seg2::Bezier { n: 2, p: ctrl },
    ];
    let placed = place_figure_2d(&figure, &[], sized(s, at))
        .unwrap()
        .segments;
    assert_eq!(placed.len(), 5);
    let Seg2::Line { a, b } = placed[0] else {
        panic!("a line stays a line")
    };
    assert!(close(a, p(100.0, 50.0)) && close(b, p(103.5, 50.0)));
    let Seg2::Arc {
        center,
        radius,
        start,
        sweep,
    } = placed[1]
    else {
        panic!("an arc stays an arc")
    };
    assert!(close(center, p(101.75, 51.75)));
    assert_eq!(
        (radius, start, sweep),
        (1.75, 0.0, PI),
        "the radius scales, the angles stay"
    );
    let Seg2::Cubic { p1, p3, .. } = placed[2] else {
        panic!("a cubic stays a cubic")
    };
    assert!(close(p1, p(100.0, 53.5)) && close(p3, p(103.5, 50.0)));
    let Seg2::Conic { apex, w, .. } = placed[3] else {
        panic!("a conic stays a conic")
    };
    assert!(close(apex, p(103.5, 53.5)));
    assert_eq!(w, FRAC_1_SQRT_2, "the weight stays");
    let Seg2::Bezier { n, p: q } = placed[4] else {
        panic!("a Bézier stays a Bézier")
    };
    assert_eq!(n, 2);
    assert!(close(q[0], p(100.0, 53.5)) && close(q[1], p(103.5, 57.0)));
    assert!(close(q[2], p(107.0, 53.5)));
    assert_eq!(q[3], p(9.0, 9.0), "the unused tail is not moved");
    assert!(place_figure_2d(&[], &[], sized(s, at))
        .unwrap()
        .segments
        .is_empty());
}

#[test]
fn sheet_figures_turn_about_their_origin() {
    let line = Seg2::Line {
        a: p(0.0, 0.0),
        b: p(1.0, 0.0),
    };
    let quarter = FigurePlacement {
        scale: 2.0,
        angle: FRAC_PI_2,
        origin: p(10.0, 20.0),
    };
    // A quarter turn: +x goes to +y, and (1, 1) to (-1, 1), twice as long, about (10, 20).
    let placed = place_figure_2d(&[line], &[p(1.0, 1.0)], quarter).unwrap();
    let Seg2::Line { a, b } = placed.segments[0] else {
        panic!("a line stays a line")
    };
    assert!(
        close(a, p(10.0, 20.0)) && close(b, p(10.0, 22.0)),
        "{a:?} {b:?}"
    );
    assert!(close(placed.points[0], p(8.0, 22.0)), "{:?}", placed.points);
    assert!(
        close(placed.direction, p(0.0, 1.0)),
        "{:?}",
        placed.direction
    );
    // A half turn: an arc's start turns by π, its radius scales and its sweep stays.
    let arc = Seg2::Arc {
        center: p(1.0, 0.0),
        radius: 1.0,
        start: 0.0,
        sweep: FRAC_PI_2,
    };
    let half = FigurePlacement {
        angle: PI,
        ..quarter
    };
    let placed = place_figure_2d(&[arc], &[p(0.0, 1.0)], half).unwrap();
    let Seg2::Arc {
        center,
        radius,
        start,
        sweep,
    } = placed.segments[0]
    else {
        panic!("an arc stays an arc")
    };
    assert!(close(center, p(8.0, 20.0)), "{center:?}");
    assert_eq!((radius, start, sweep), (2.0, PI, FRAC_PI_2));
    assert!(
        close(placed.points[0], p(10.0, 18.0)),
        "{:?}",
        placed.points
    );
    assert!(
        close(placed.direction, p(-1.0, 0.0)),
        "{:?}",
        placed.direction
    );
    // π/6 with a clockwise arc: the centre (2, 0) goes to (√3, 1), and the arc's placed start
    // point is the original start point placed.
    let r3 = 3.0_f64.sqrt();
    let arc = Seg2::Arc {
        center: p(2.0, 0.0),
        radius: 0.5,
        start: PI / 4.0,
        sweep: -PI,
    };
    let sixth = FigurePlacement {
        scale: 1.0,
        angle: PI / 6.0,
        origin: p(0.0, 0.0),
    };
    let from = p(2.0 + 0.5 * (PI / 4.0).cos(), 0.5 * (PI / 4.0).sin());
    let placed = place_figure_2d(&[arc], &[from], sixth).unwrap();
    let Seg2::Arc {
        center,
        radius,
        start,
        sweep,
    } = placed.segments[0]
    else {
        panic!("an arc stays an arc")
    };
    assert!(close(center, p(r3, 1.0)), "{center:?}");
    assert_eq!((radius, sweep), (0.5, -PI));
    assert!((start - (PI / 4.0 + PI / 6.0)).abs() <= 1e-15, "{start}");
    let start_point = center + p(start.cos(), start.sin()) * radius;
    assert!(
        close(start_point, placed.points[0]),
        "{start_point:?} {:?}",
        placed.points
    );
    assert!(
        close(placed.direction, p(r3 / 2.0, 0.5)),
        "{:?}",
        placed.direction
    );
    // A full turn is no turn, and no figure still places its points.
    let full = FigurePlacement {
        angle: TAU,
        ..sixth
    };
    let placed = place_figure_2d(&[], &[p(1.0, 2.0)], full).unwrap();
    assert!(placed.segments.is_empty());
    assert!(close(placed.points[0], p(1.0, 2.0)) && close(placed.direction, p(1.0, 0.0)));
}

#[test]
fn sheet_figures_refuse_a_hostile_placing() {
    let o = p(0.0, 0.0);
    let line = [Seg2::Line {
        a: o,
        b: p(1.0, 0.0),
    }];
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            place_figure_2d(&line, &[], sized(scale, o)).is_err(),
            "scale {scale}: no mirror, no collapse"
        );
    }
    assert!(place_figure_2d(&line, &[], sized(1.0, p(f64::NAN, 0.0))).is_err());
    for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let turned = FigurePlacement {
            angle,
            ..sized(1.0, o)
        };
        assert!(
            place_figure_2d(&line, &[], turned).is_err(),
            "angle {angle}"
        );
        let nothing = place_figure_2d(&[], &[], turned);
        assert!(nothing.is_err(), "angle {angle}: no direction from it");
    }
    assert!(place_figure_2d(&[], &[p(f64::NAN, 0.0)], sized(1.0, o)).is_err());
    let far = place_figure_2d(&[], &[p(1e308, 0.0)], sized(10.0, o));
    assert!(far.is_err(), "a point that overflows");
    let nan = [Seg2::Arc {
        center: o,
        radius: f64::NAN,
        start: 0.0,
        sweep: 1.0,
    }];
    assert!(place_figure_2d(&nan, &[], sized(1.0, o)).is_err());
    let big = [Seg2::Line {
        a: o,
        b: p(1e308, 0.0),
    }];
    assert!(
        place_figure_2d(&big, &[], sized(10.0, o)).is_err(),
        "a point that overflows"
    );
    for n in [0, 10] {
        let bezier = [Seg2::Bezier {
            n,
            p: [o; MAX_BEZIER_CTRL],
        }];
        assert!(
            place_figure_2d(&bezier, &[], sized(1.0, o)).is_err(),
            "degree {n}"
        );
    }
    let many = vec![line[0]; MAX_SEGMENTS + 1];
    assert!(place_figure_2d(&many, &[], sized(1.0, o)).is_err());
    let points = vec![o; MAX_SEGMENTS + 1];
    assert!(place_figure_2d(&[], &points, sized(1.0, o)).is_err());
    assert_eq!(
        place_figure_2d(&[], &points[1..], sized(1.0, o))
            .unwrap()
            .points
            .len(),
        MAX_SEGMENTS
    );
    assert_eq!(
        place_figure_2d(&many[1..], &[], sized(1.0, o))
            .unwrap()
            .segments
            .len(),
        MAX_SEGMENTS
    );
}

#[test]
fn sheet_figures_regular_polygons_have_their_closed_form_corners() {
    // A triangle of side 6 about (100, 50), point up: circumradius 6 / (2 sin 60°) = 2√3.
    let r3 = 3.0_f64.sqrt();
    let tri = regular_polygon_2d(p(100.0, 50.0), 3, 6.0, FRAC_PI_2).unwrap();
    let want = [
        p(100.0, 50.0 + 2.0 * r3),
        p(97.0, 50.0 - r3),
        p(103.0, 50.0 - r3),
    ];
    assert_eq!(tri.len(), 3);
    for (got, want) in tri.iter().zip(want) {
        assert!(close(*got, want), "{got:?} is not {want:?}");
    }
    // A square of side 2 turned 45°: its corners on the diagonals, counter-clockwise.
    let square = regular_polygon_2d(p(0.0, 0.0), 4, 2.0, PI / 4.0).unwrap();
    let want = [p(1.0, 1.0), p(-1.0, 1.0), p(-1.0, -1.0), p(1.0, -1.0)];
    for (got, want) in square.iter().zip(want) {
        assert!(close(*got, want), "{got:?} is not {want:?}");
    }
    // A hexagon's circumradius is its side.
    let hex = regular_polygon_2d(p(0.0, 0.0), 6, 1.0, 0.0).unwrap();
    assert_eq!(hex.len(), 6);
    for (k, got) in hex.iter().enumerate() {
        let a = k as f64 * PI / 3.0;
        assert!(close(*got, p(a.cos(), a.sin())), "corner {k}: {got:?}");
    }
}

#[test]
fn sheet_figures_refuse_a_hostile_regular_polygon() {
    let c = p(0.0, 0.0);
    for sides in [0, 1, 2, MAX_SEGMENTS + 1] {
        assert!(
            regular_polygon_2d(c, sides, 1.0, 0.0).is_err(),
            "{sides} sides"
        );
    }
    for side in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e308] {
        assert!(regular_polygon_2d(c, 3, side, 0.0).is_err(), "side {side}");
    }
    assert!(regular_polygon_2d(p(f64::INFINITY, 0.0), 3, 1.0, 0.0).is_err());
    assert!(regular_polygon_2d(c, 3, 1.0, f64::NAN).is_err());
    assert_eq!(
        regular_polygon_2d(c, MAX_SEGMENTS, 1.0, 0.0).unwrap().len(),
        MAX_SEGMENTS
    );
    // The bound is a profile's, compared the same way (`<`): check_regions refuses a point at
    // MAX_PROFILE_COORD and takes one a unit inside it.
    let room = |x: f64| check_regions([&rectangle(p(0.0, 0.0), p(x, 1.0))].into_iter(), 1e-7);
    assert_eq!(room(MAX_PROFILE_COORD), Err(ProfileRefusal::LineOrArc));
    assert_eq!(room(MAX_PROFILE_COORD - 1.0), Ok(()));
    // A hexagon of side 1 has its first corner on +x, its circumradius (1 and a rounding) from
    // its centre. From a centre 1 inside the bound, the sum rounds to the bound exactly.
    let radius = 1.0 / (2.0 * (PI / 6.0).sin());
    assert_eq!(MAX_PROFILE_COORD - 1.0 + radius, MAX_PROFILE_COORD);
    let hexagon = |x: f64| regular_polygon_2d(p(x, 0.0), 6, 1.0, 0.0);
    assert!(
        hexagon(MAX_PROFILE_COORD - 1.0).is_err(),
        "a corner at the bound"
    );
    assert!(
        hexagon(MAX_PROFILE_COORD - 0.5).is_err(),
        "a corner just past it"
    );
    assert!(
        hexagon(MAX_PROFILE_COORD - 2.0).is_ok(),
        "a corner a unit inside it"
    );
}

fn square10() -> [Point2; 4] {
    [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)]
}

fn fits(fit: Option<BoxFit>, height: f64, min: Point2) {
    let fit = fit.expect("a box fits");
    let off = (fit.height - height).abs();
    assert!(
        off <= 1e-12 * (1.0 + height),
        "height {} is not {height}",
        fit.height
    );
    assert!(close(fit.min, min), "min {:?} is not {min:?}", fit.min);
}

#[test]
fn sheet_figures_a_label_box_fits_a_square_outline_in_closed_form() {
    let fit = |cx, aspect, margin, top| {
        box_fit_in_convex_polygon(&square10(), cx, aspect, margin, top).unwrap()
    };
    // The square [0, 10]², a box twice as wide as tall, a margin of 1: the sides leave 8 across,
    // so the height is 4, its bottom on the margin line y = 1.
    fits(fit(5.0, 2.0, 1.0, 100.0), 4.0, p(1.0, 1.0));
    // The height cap holds, and the bottom stays as low as it can.
    fits(fit(5.0, 2.0, 1.0, 3.0), 3.0, p(2.0, 1.0));
    // Off centre: the line x = 3 is 2 from the left side's margin, so the box is 4 wide.
    fits(fit(3.0, 2.0, 1.0, 100.0), 2.0, p(1.0, 1.0));
    // A margin of 5 leaves only the centre point, a box of no height; 6 leaves nothing.
    fits(fit(5.0, 2.0, 5.0, 100.0), 0.0, p(5.0, 5.0));
    assert_eq!(fit(5.0, 2.0, 6.0, 100.0), None);
    // A box of no width (an empty label) is held by the bottom and top margins alone.
    fits(fit(5.0, 0.0, 1.0, 100.0), 8.0, p(5.0, 1.0));
    // A centre line outside the outline fits nothing.
    assert_eq!(fit(12.0, 2.0, 1.0, 100.0), None);
}

#[test]
fn sheet_figures_a_label_box_fits_a_triangle_in_closed_form() {
    // The triangle of side 6 about (100, 50), point up, with a margin of 0.6 square to each edge:
    // the margin lines make the triangle of inradius √3 - 0.6, so of side s' = 6 - 2√3 · 0.6, its
    // base at y = 50 - √3 + 0.6. A box of aspect a on that base, centred, reaches the slopes when
    // a h / 2 = s' / 2 - h / √3, so h = s' / (a + 2 / √3).
    let r3 = 3.0_f64.sqrt();
    let tri = regular_polygon_2d(p(100.0, 50.0), 3, 6.0, FRAC_PI_2).unwrap();
    let (a, m) = (2.0, 0.6);
    let s = 6.0 - 2.0 * r3 * m;
    let h = s / (a + 2.0 / r3);
    let base = 50.0 - r3 + m;
    let fit = |top| box_fit_in_convex_polygon(&tri, 100.0, a, m, top).unwrap();
    fits(fit(100.0), h, p(100.0 - a * h / 2.0, base));
    // Capped below that, the box sits on the base, not anywhere between the slopes.
    fits(fit(1.0), 1.0, p(99.0, base));
}

#[test]
fn sheet_figures_a_label_box_on_a_pointed_bottom_sits_on_the_higher_of_its_bounds() {
    // Two edges below the box at 45°, meeting at a point that is not under the centre line: each
    // bounds the bottom through its own lower corner, and the box sits on the higher bound. With a
    // box as wide as tall, centred on x = cx: the left edge (x + y = k) needs y0 ≥ k - cx + h / 2,
    // the right one (y = x - j) needs y0 ≥ cx - j + h / 2, and the top y = t needs y0 ≤ t - h.
    let fit = |poly: &[Point2], cx| box_fit_in_convex_polygon(poly, cx, 1.0, 0.0, 100.0).unwrap();
    // The point at (1, 0), sides x = 0 and x = 4, top y = 10, centre x = 2: the sides give h = 4,
    // the right edge y0 ≥ 1 + h / 2 = 3, above the left edge's y0 ≥ h / 2 - 1 = 1.
    let narrow = [
        p(1.0, 0.0),
        p(4.0, 3.0),
        p(4.0, 10.0),
        p(0.0, 10.0),
        p(0.0, 1.0),
    ];
    fits(fit(&narrow, 2.0), 4.0, p(0.0, 3.0));
    // The point at (5, 0), sides x = 0 and x = 20, top y = 20, centre x = 8: the sides allow 16,
    // and the right edge and the top meet first, at 3 + h / 2 = 20 - h, so h = 34 / 3 with its
    // bottom at 26 / 3; the left edge's bound, h / 2 - 3, is 6 lower.
    let wide = [
        p(5.0, 0.0),
        p(20.0, 15.0),
        p(20.0, 20.0),
        p(0.0, 20.0),
        p(0.0, 5.0),
    ];
    let h = 34.0 / 3.0;
    fits(fit(&wide, 8.0), h, p(8.0 - h / 2.0, 26.0 / 3.0));
}

#[test]
fn sheet_figures_refuse_an_outline_that_is_not_convex_and_counter_clockwise() {
    let ok = |poly: &[Point2]| box_fit_in_convex_polygon(poly, 5.0, 2.0, 1.0, 100.0);
    let mut clockwise = square10();
    clockwise.reverse();
    assert!(ok(&clockwise).is_err(), "clockwise");
    let dart = [
        p(0.0, 0.0),
        p(10.0, 0.0),
        p(5.0, 2.0),
        p(10.0, 10.0),
        p(0.0, 10.0),
    ];
    assert!(ok(&dart).is_err(), "not convex");
    // A pentagram turns left at every corner, but it winds twice.
    let star: Vec<Point2> = (0..5)
        .map(|k| f64::from(k * 2) * TAU / 5.0)
        .map(|a| p(5.0 + 4.0 * a.cos(), 5.0 + 4.0 * a.sin()))
        .collect();
    assert!(ok(&star).is_err(), "a pentagram");
    assert!(ok(&square10()[..2]).is_err(), "two corners");
    let repeated = [
        p(0.0, 0.0),
        p(10.0, 0.0),
        p(10.0, 0.0),
        p(10.0, 10.0),
        p(0.0, 10.0),
    ];
    assert!(ok(&repeated).is_err(), "an edge of no length");
    let mut nan = square10();
    nan[2] = p(f64::NAN, 10.0);
    assert!(ok(&nan).is_err());
    for (cx, aspect, margin, top) in [
        (f64::NAN, 2.0, 1.0, 1.0),
        (5.0, -1.0, 1.0, 1.0),
        (5.0, f64::INFINITY, 1.0, 1.0),
        (5.0, 2.0, -0.1, 1.0),
        (5.0, 2.0, 1.0, -1.0),
        (5.0, 2.0, 1.0, f64::INFINITY),
    ] {
        let got = box_fit_in_convex_polygon(&square10(), cx, aspect, margin, top);
        assert!(got.is_err(), "{cx} {aspect} {margin} {top}");
    }
    let many = vec![p(0.0, 0.0); MAX_SEGMENTS + 1];
    assert!(ok(&many).is_err());
}
