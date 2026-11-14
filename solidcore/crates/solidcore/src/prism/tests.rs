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
    check_regions, curve_on_frame, line_coords_2d, tapered_flange_ends, to_local, to_local_all,
    Loop2, ProfileRefusal, Region2, Seg2, MAX_SEGMENTS, MIN_DIRECTION, VIEW_FRAME_TOL,
};
use super::read::{planar_curves, planar_pieces, PlanarPiece, Run};
use super::sheet::{region_loops, sheet_regions};
use crate::boolean::api_brep::{boolean_2d, Boolean2dOutcome};
use crate::curve::curve::{Curve, CurveKind};
use crate::eval::Frame;
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::Point3;
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

/// Whether `r` is a refusal whose message names `size`, the size that fits, as §7 of
/// `docs/definitions-and-certificates.md` asks of a refusal.
fn says<T, E: std::fmt::Display>(r: Result<T, E>, size: &str) -> bool {
    r.is_err_and(|e| e.to_string().contains(size))
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

// ---- A view's points in its frame, and points along a line on it ----

fn tilted() -> Frame {
    // x along (0.6, 0.8, 0) and y along +z: exactly unit and square, so the closed forms are exact.
    Frame {
        origin: [1.0, 2.0, 3.0],
        x_axis: [0.6, 0.8, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    }
}

/// Each point reads as `to_local` reads it, in its order: the closed forms on a tilted frame.
#[test]
fn to_local_all_reads_each_point_as_to_local_does() {
    let f = tilted();
    let points = [
        Point3::new(1.6, 2.8, 3.0),
        Point3::new(1.0, 2.0, 5.0),
        Point3::new(-2.0, -2.0, 0.0),
    ];
    let got = to_local_all(&f, &points).unwrap();
    assert_eq!(got.len(), 3);
    for (g, q) in got.iter().zip(&points) {
        assert_eq!(*g, to_local(&f, *q));
    }
    assert!((got[0].x - 1.0).abs() < 1e-15 && got[0].y.abs() < 1e-15);
    assert!(got[1].x.abs() < 1e-15 && (got[1].y - 2.0).abs() < 1e-15);
    assert!((got[2].x + 5.0).abs() < 1e-14 && (got[2].y + 3.0).abs() < 1e-14);
    assert!(to_local_all(&f, &[]).unwrap().is_empty());
}

/// A frame as close to orthonormal as SolidCraft accepts a view's is read; one further off, or
/// not finite, is refused, and so are points not finite and too many points. A refusal of a size
/// names the size that fits.
#[test]
fn to_local_all_takes_a_views_frame_and_refuses_the_rest() {
    let off = |dx: f64| Frame {
        x_axis: [1.0 + dx, 0.0, 0.0],
        ..xy()
    };
    let q = [Point3::new(1.0, 1.0, 0.0)];
    assert!(to_local_all(&off(1e-10), &q).is_ok());
    assert!(to_local_all(&off(0.5 * VIEW_FRAME_TOL), &q).is_ok());
    assert!(says(
        to_local_all(&off(2.0 * VIEW_FRAME_TOL), &q),
        &format!("{VIEW_FRAME_TOL:e}")
    ));
    assert!(to_local_all(
        &Frame {
            origin: [f64::NAN, 0.0, 0.0],
            ..xy()
        },
        &q
    )
    .is_err());
    assert!(to_local_all(&xy(), &[Point3::new(f64::NAN, 0.0, 0.0)]).is_err());
    assert!(to_local_all(&xy(), &[Point3::new(f64::INFINITY, 0.0, 0.0)]).is_err());
    // Finite points 1e12 out read finitely.
    assert!(to_local_all(&xy(), &[Point3::new(1e12, -1e12, 1e12)]).is_ok());
    let many = vec![Point3::new(0.0, 0.0, 0.0); MAX_SEGMENTS + 1];
    assert!(says(
        to_local_all(&xy(), &many),
        &format!("at most {MAX_SEGMENTS}")
    ));
    assert!(to_local_all(&xy(), &many[..MAX_SEGMENTS]).is_ok());
}

/// Along and across a 45° line, by hand; the unit and the normal it turns to; no points gives the
/// line's unit and normal alone; the other way along the line flips both readings.
#[test]
fn line_coords_2d_reads_along_and_across_a_line() {
    let h = FRAC_1_SQRT_2;
    let got = line_coords_2d(
        p(1.0, 1.0),
        Vec2::new(2.0, 2.0),
        &[p(3.0, 1.0), p(1.0, 1.0), p(0.0, 2.0)],
    )
    .unwrap();
    assert!((got.unit.x - h).abs() < 1e-15 && (got.unit.y - h).abs() < 1e-15);
    assert!((got.normal.x + h).abs() < 1e-15 && (got.normal.y - h).abs() < 1e-15);
    let want = [
        (2f64.sqrt(), -(2f64.sqrt())),
        (0.0, 0.0),
        (0.0, 2f64.sqrt()),
    ];
    for (g, (along, across)) in got.coords.iter().zip(want) {
        assert!(
            (g.x - along).abs() < 1e-14 && (g.y - across).abs() < 1e-14,
            "{g:?}"
        );
    }
    let back = line_coords_2d(p(1.0, 1.0), Vec2::new(-2.0, -2.0), &[p(3.0, 1.0)]).unwrap();
    assert!((back.coords[0].x + got.coords[0].x).abs() < 1e-15);
    assert!((back.coords[0].y + got.coords[0].y).abs() < 1e-15);
    let alone = line_coords_2d(p(0.0, 0.0), Vec2::new(0.0, -3.0), &[]).unwrap();
    assert_eq!(
        (alone.unit, alone.normal),
        (Vec2::new(0.0, -1.0), Vec2::new(1.0, 0.0))
    );
    assert!(alone.coords.is_empty());
}

/// The direction's length decides at `MIN_DIRECTION` exactly, and huge and tiny directions give
/// the right unit or a refusal, never a zero "unit": the length is scaled before it is squared.
#[test]
fn line_coords_2d_takes_its_unit_at_the_thresholds() {
    let unit = |d: Vec2| line_coords_2d(p(0.0, 0.0), d, &[]).map(|l| l.unit);
    assert!(unit(Vec2::new(0.0, 0.0)).is_err());
    assert!(says(
        unit(Vec2::new(MIN_DIRECTION, 0.0)),
        &format!("{MIN_DIRECTION:e}")
    ));
    assert_eq!(
        unit(Vec2::new(1.000_000_1 * MIN_DIRECTION, 0.0)).unwrap(),
        Vec2::new(1.0, 0.0)
    );
    let big = unit(Vec2::new(1e200, 1e200)).unwrap();
    assert!((big.x - FRAC_1_SQRT_2).abs() < 1e-15 && (big.y - FRAC_1_SQRT_2).abs() < 1e-15);
    let huge = unit(Vec2::new(-f64::MAX, f64::MAX)).unwrap();
    assert!((huge.x + FRAC_1_SQRT_2).abs() < 1e-15 && (huge.y - FRAC_1_SQRT_2).abs() < 1e-15);
    assert!(unit(Vec2::new(1e-300, 1e-300)).is_err());
    assert!(unit(Vec2::new(f64::NAN, 1.0)).is_err());
    assert!(unit(Vec2::new(f64::INFINITY, 0.0)).is_err());
}

/// What a line can't read is refused: an origin or a point not finite, readings that overflow,
/// too many points.
#[test]
fn line_coords_2d_refuses_what_it_cannot_read() {
    let d = Vec2::new(1.0, 0.0);
    assert!(line_coords_2d(p(f64::NAN, 0.0), d, &[]).is_err());
    assert!(line_coords_2d(p(0.0, 0.0), d, &[p(0.0, f64::INFINITY)]).is_err());
    assert!(line_coords_2d(p(-f64::MAX, 0.0), d, &[p(f64::MAX, 0.0)]).is_err());
    let many = vec![p(0.0, 0.0); MAX_SEGMENTS + 1];
    assert!(says(
        line_coords_2d(p(0.0, 0.0), d, &many),
        &format!("at most {MAX_SEGMENTS}")
    ));
    assert!(line_coords_2d(p(0.0, 0.0), d, &many[..MAX_SEGMENTS]).is_ok());
}
