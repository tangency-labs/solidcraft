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
use super::profile::{
    check_regions, curve_on_frame, tapered_flange_ends, Loop2, ProfileRefusal, Region2, Seg2,
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

mod path_export_regressions {
    use super::super::path_export::*;
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
            round_trip: |v| Some(if v.abs() < 1e-12 { 0.0 } else { v }),
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

    fn scalar_encoding(round_trip: fn(f64) -> Option<f64>) -> ScalarEncoding {
        ScalarEncoding {
            round_trip,
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
                path_shapes(DxfPath::Open(&segs), DxfPathStyle::Exact, encoding).unwrap_err();
            assert_eq!(error.piece, Some((1, 2)));
            assert_eq!(error.reason, "line endpoint cannot be encoded");
        }
    }

    #[test]
    fn refused_arc_angle_never_certifies_a_one_sided_encoding() {
        let arc = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 2.0,
            start: 0.0,
            sweep: std::f64::consts::FRAC_PI_2,
        };
        for encoding in [
            scalar_encoding(|v| (v != 90.0).then_some(v)),
            scalar_encoding(|v| (v != 0.0).then_some(v)),
        ] {
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
