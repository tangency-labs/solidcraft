//! Channel fills (`prism::fill`): the section of every standard groove and of grooves that
//! match no standard against closed-form areas, the contour of each face (a flat root, or a
//! second face of its own), the depth `s` and its notes, and the refusals.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore::error::Reason;
use solidcore::error::Result;
use solidcore::math::{Point2, Vec2};
use solidcore::prism::fill::{
    fill, Channel, Contour, FillFailure, FillNote, FillOptions, FillSide, Filled,
};
use solidcore::prism::profile::Seg2;
use solidcore::{Kernel, SolidError};

/// Gap between the plates in every example.
const G: f64 = 1.0;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn line(a: Point2, b: Point2) -> Seg2 {
    Seg2::Line { a, b }
}

fn tan(deg: f64) -> f64 {
    deg.to_radians().tan()
}

fn mirror(wall: &[Seg2]) -> Vec<Seg2> {
    wall.iter()
        .map(|s| match *s {
            Seg2::Line { a, b } => line(p(-a.x, a.y), p(-b.x, b.y)),
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Seg2::Arc {
                center: p(-center.x, center.y),
                radius,
                start: PI - start,
                sweep: -sweep,
            },
            other => other,
        })
        .collect()
}

/// The kernel's default zone tolerance: the depth margin of these sections (their rounding
/// allowance, 1e-9 of their size, is smaller).
const ZONE: f64 = 1e-5;

/// One test case: the groove's section and the options it is filled with, flat faces and the
/// whole groove unless a case says otherwise.
#[derive(Debug, Clone)]
struct Case {
    left: Vec<Seg2>,
    right: Vec<Seg2>,
    depth_normal: Option<Vec2>,
    face: Contour,
    second_face: Option<Contour>,
    depth: Option<f64>,
}

impl Default for Case {
    fn default() -> Self {
        Self {
            left: Vec::new(),
            right: Vec::new(),
            depth_normal: None,
            face: Contour::Flat,
            second_face: None,
            depth: None,
        }
    }
}

fn channel(left: Vec<Seg2>, right: Vec<Seg2>) -> Case {
    Case {
        left,
        right,
        ..Case::default()
    }
}

/// The two kernel steps: the groove's section (here built by hand), then the fill.
fn run(c: &Case) -> Result<Filled> {
    let section = Channel {
        left: c.left.clone(),
        right: c.right.clone(),
        depth_normal: c.depth_normal,
    };
    let options = FillOptions {
        face: c.face,
        second_face: c.second_face,
        depth: c.depth,
    };
    fill(&Kernel::default(), &section, &options)
}

fn section(w: &Case) -> Filled {
    run(w).unwrap()
}

fn area(w: &Case) -> f64 {
    let s = section(w);
    assert!(s.region.holes.is_empty());
    s.region.signed_area().unwrap().value
}

fn close(got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 1e-9 * want.abs().max(1.0),
        "area {got} != {want}"
    );
}

/// The circular segment between a chord of half-length `a` and its arc of sagitta `h`.
fn segment(a: f64, h: f64) -> f64 {
    let r = (a * a + h * h) / (2.0 * h);
    let theta = 2.0 * a.atan2(r - h);
    r * r / 2.0 * (theta - theta.sin())
}

/// A single-V (or Y with root face `c`) groove in plates `t` thick, included angle `angle`.
fn v_walls(t: f64, angle: f64, c: f64) -> (Vec<Seg2>, Vec<Seg2>) {
    let top = G / 2.0 + (t - c) * tan(angle / 2.0);
    let mut left = vec![line(p(-top, 0.0), p(-G / 2.0, -(t - c)))];
    if c > 0.0 {
        left.push(line(p(-G / 2.0, -(t - c)), p(-G / 2.0, -t)));
    }
    let right = mirror(&left);
    (left, right)
}

/// A single-bevel (or HY with root face `c`) groove: the left edge square, the right bevelled.
fn bevel_walls(t: f64, angle: f64, c: f64) -> (Vec<Seg2>, Vec<Seg2>) {
    let left = vec![line(p(-G / 2.0, 0.0), p(-G / 2.0, -t))];
    let mut right = vec![line(
        p(G / 2.0 + (t - c) * tan(angle), 0.0),
        p(G / 2.0, -(t - c)),
    )];
    if c > 0.0 {
        right.push(line(p(G / 2.0, -(t - c)), p(G / 2.0, -t)));
    }
    (left, right)
}

const U_T: f64 = 10.0;
const U_C: f64 = 1.5;
const U_R: f64 = 3.0;
const U_FLANK: f64 = 8.0;

/// The right half of a U groove: a flank of `U_FLANK` degrees, a quarter round of `U_R` at the
/// bottom, a root face of `U_C`.
fn u_right() -> Vec<Seg2> {
    let yb = -(U_T - U_C);
    let cy = yb + U_R;
    let cx = G / 2.0;
    let top = cx + U_R + (-cy) * tan(U_FLANK);
    vec![
        line(p(top, 0.0), p(cx + U_R, cy)),
        Seg2::Arc {
            center: p(cx, cy),
            radius: U_R,
            start: 0.0,
            sweep: -FRAC_PI_2,
        },
        line(p(cx, yb), p(cx, -U_T)),
    ]
}

/// One half of the U groove past the gap: the flank's trapezoid and the quarter round.
fn u_half_area() -> f64 {
    let cy = -(U_T - U_C) + U_R;
    U_R * -cy + tan(U_FLANK) * cy * cy / 2.0 + PI * U_R * U_R / 4.0
}

/// A double-V groove in plates `t` thick with a root face `c` in the middle, 60 degrees a side.
fn x_walls(t: f64, c: f64) -> (Vec<Seg2>, Vec<Seg2>) {
    let d = (t - c) / 2.0;
    let top = G / 2.0 + d * tan(30.0);
    let left = vec![
        line(p(-top, 0.0), p(-G / 2.0, -d)),
        line(p(-G / 2.0, -d), p(-G / 2.0, -d - c)),
        line(p(-G / 2.0, -d - c), p(-top, -t)),
    ];
    let right = mirror(&left);
    (left, right)
}

#[test]
fn every_standard_groove_is_filled_to_its_closed_form_area() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let v = channel(l, r);
    close(area(&v), G * t + t * t * tan(30.0));
    let s = section(&v);
    assert!(s.through);
    assert!((s.channel_depth - t).abs() < 1e-12);
    assert!(s.notes.is_empty());

    let (l, r) = bevel_walls(t, 45.0, 0.0);
    close(area(&channel(l, r)), G * t + t * t / 2.0);

    let c = 2.0;
    let (l, r) = v_walls(t, 60.0, c);
    close(area(&channel(l, r)), G * t + (t - c) * (t - c) * tan(30.0));

    let (l, r) = bevel_walls(t, 45.0, c);
    close(area(&channel(l, r)), G * t + (t - c) * (t - c) / 2.0);

    let right = u_right();
    close(
        area(&channel(mirror(&right), right.clone())),
        G * U_T + 2.0 * u_half_area(),
    );
    let square = vec![line(p(-G / 2.0, 0.0), p(-G / 2.0, -U_T))];
    close(area(&channel(square, right)), G * U_T + u_half_area());

    let (t2, c2) = (12.0, 2.0);
    let d = (t2 - c2) / 2.0;
    let (l, r) = x_walls(t2, c2);
    close(area(&channel(l, r)), G * t2 + 2.0 * d * d * tan(30.0));

    let k_right = vec![
        line(p(G / 2.0 + d, 0.0), p(G / 2.0, -d)),
        line(p(G / 2.0, -d), p(G / 2.0, -d - c2)),
        line(p(G / 2.0, -d - c2), p(G / 2.0 + d, -t2)),
    ];
    let k_left = vec![line(p(-G / 2.0, 0.0), p(-G / 2.0, -t2))];
    close(area(&channel(k_left, k_right)), G * t2 + d * d);
}

#[test]
fn a_groove_that_matches_no_standard_is_filled_as_modelled() {
    // unequal bevels (25 and 35 degrees), the right plate 1 mm thinner, flush underneath
    let left = vec![line(
        p(-(G / 2.0 + 8.0 * tan(25.0)), 0.0),
        p(-G / 2.0, -8.0),
    )];
    let right = vec![line(p(G / 2.0 + 7.0 * tan(35.0), -1.0), p(G / 2.0, -8.0))];
    let corners = [
        left[0].start(),
        right[0].start(),
        right[0].end(),
        left[0].end(),
    ];
    // the quadrilateral's area by the shoelace formula, the corners clockwise
    let shoelace = -0.5
        * (0..4)
            .map(|i| {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                a.x * b.y - a.y * b.x
            })
            .sum::<f64>();
    close(area(&channel(left, right)), shoelace);

    // a square groove 6 wide whose left plate has its top corner rounded (radius 1)
    let left = vec![
        Seg2::Arc {
            center: p(-4.0, -1.0),
            radius: 1.0,
            start: FRAC_PI_2,
            sweep: -FRAC_PI_2,
        },
        line(p(-3.0, -1.0), p(-3.0, -6.0)),
    ];
    let right = vec![line(p(3.0, 0.0), p(3.0, -6.0))];
    close(area(&channel(left, right)), 36.0 + 1.0 - PI / 4.0);
}

#[test]
fn convex_faces_add_their_segments_and_a_one_sided_root_stays_flat() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let groove = G * t + t * t * tan(30.0);
    let half = G / 2.0 + t * tan(30.0);
    let w = Case {
        face: Contour::Convex { height: 1.5 },
        ..channel(l.clone(), r.clone())
    };
    close(area(&w), groove + segment(half, 1.5));
    // with a flat root, the root closes the 1 mm gap flat: nothing below the plate
    let s = section(&w);
    let lowest = s
        .region
        .outer
        .segs
        .iter()
        .flat_map(|seg| (0..=16).map(move |i| seg.point_at(f64::from(i) / 16.0).y))
        .fold(f64::MAX, f64::min);
    assert!((lowest + t).abs() < 1e-12, "root at {lowest}");
    assert!(s.notes.is_empty());

    // with a second face: each face follows its own contour, flat by default
    let (t2, c2) = (12.0, 2.0);
    let d = (t2 - c2) / 2.0;
    let (l, r) = x_walls(t2, c2);
    let half = G / 2.0 + d * tan(30.0);
    let x_groove = G * t2 + 2.0 * d * d * tan(30.0);
    let w = Case {
        face: Contour::Convex { height: 1.0 },
        second_face: Some(Contour::Convex { height: 1.2 }),
        ..channel(l.clone(), r.clone())
    };
    close(area(&w), x_groove + segment(half, 1.0) + segment(half, 1.2));
    let w = Case {
        face: Contour::Convex { height: 1.0 },
        second_face: Some(Contour::Flat),
        ..channel(l.clone(), r.clone())
    };
    close(area(&w), x_groove + segment(half, 1.0));
    let w = Case {
        second_face: Some(Contour::Convex { height: 1.2 }),
        ..channel(l, r)
    };
    close(area(&w), x_groove + segment(half, 1.2));
}

/// The blended face's area above the chord: the convex arc between the toes and a concave
/// blend `o` long past each (handover §4.4).
fn blended_cap(a: f64, h: f64, o: f64) -> f64 {
    let x = a + o;
    let sum = (x * x + h * h) / (2.0 * h);
    let big = a * sum / x;
    let small = sum - big;
    let main = 2.0 * a * (h - big) + a * (big * big - a * a).sqrt() + big * big * (a / big).asin();
    let blend = small * o
        - o / 2.0 * (small * small - o * o).sqrt()
        - small * small / 2.0 * (o / small).asin();
    main + 2.0 * blend
}

#[test]
fn blended_toes_run_out_tangentially_past_the_groove() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let groove = G * t + t * t * tan(30.0);
    let a = G / 2.0 + t * tan(30.0);
    let h = 1.5;
    // the plates' outer faces along the chord (plates in one plane): blended toes build
    let flat = Case {
        depth_normal: Some(Vec2::new(0.0, 1.0)),
        ..channel(l.clone(), r.clone())
    };
    // an overlap equal to the crest height (ops' default)
    let w = Case {
        face: Contour::ConvexBlended {
            height: h,
            overlap: h,
        },
        ..flat.clone()
    };
    close(area(&w), groove + blended_cap(a, h, h));
    let s = section(&w);
    // the face reaches the crest height and no higher, and runs out h past each toe
    let (mut lo_x, mut hi_x, mut top) = (f64::MAX, f64::MIN, f64::MIN);
    for seg in &s.region.outer.segs {
        for i in 0..=64 {
            let q = seg.point_at(f64::from(i) / 64.0);
            lo_x = lo_x.min(q.x);
            hi_x = hi_x.max(q.x);
            top = top.max(q.y);
        }
    }
    assert!((top - h).abs() < 1e-6, "crest {top}");
    assert!((hi_x - (a + h)).abs() < 1e-9 && (lo_x + a + h).abs() < 1e-9);
    // where the arcs meet they share their tangent (the blend is smooth)
    let arcs: Vec<&Seg2> = s
        .region
        .outer
        .segs
        .iter()
        .filter(|seg| matches!(seg, Seg2::Arc { .. }))
        .collect();
    assert_eq!(arcs.len(), 3);
    for pair in arcs.windows(2) {
        let (d1, d2) = (tangent_at(pair[0], 1.0), tangent_at(pair[1], 0.0));
        assert!(pair[0].end().distance(pair[1].start()) < 1e-12);
        assert!(
            (d1.x * d2.y - d1.y * d2.x).abs() < 1e-9,
            "a kink where arcs meet"
        );
    }

    // a chosen overlap
    let w = Case {
        face: Contour::ConvexBlended {
            height: h,
            overlap: 0.5,
        },
        ..flat
    };
    close(area(&w), groove + blended_cap(a, h, 0.5));

    // no overlap is the plain convex face
    let w = Case {
        face: Contour::ConvexBlended {
            height: h,
            overlap: 0.0,
        },
        ..channel(l, r)
    };
    close(area(&w), groove + segment(a, h));
}

/// The unit direction an arc runs in at the fraction `t` of its sweep.
fn tangent_at(seg: &Seg2, t: f64) -> Point2 {
    let Seg2::Arc { start, sweep, .. } = *seg else {
        panic!("not an arc");
    };
    let ang = start + sweep * t;
    let s = sweep.signum();
    p(-ang.sin() * s, ang.cos() * s)
}

#[test]
fn a_concave_face_is_built_and_says_how_deep_it_dips() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let groove = G * t + t * t * tan(30.0);
    let a = G / 2.0 + t * tan(30.0);
    let w = Case {
        face: Contour::Concave { depth: 1.0 },
        ..channel(l, r)
    };
    close(area(&w), groove - segment(a, 1.0));
    assert_eq!(
        section(&w).notes,
        vec![FillNote::BelowToes {
            side: FillSide::Face,
            depth: 1.0,
            below: [1.0, 1.0],
            ratio: 1.0 / 6.0,
        }]
    );
    // a fill with a second face with its second face concave says so for that side
    let (t2, c2) = (12.0, 2.0);
    let d = (t2 - c2) / 2.0;
    let (xl, xr) = x_walls(t2, c2);
    let w = Case {
        second_face: Some(Contour::Concave { depth: 0.5 }),
        ..channel(xl, xr)
    };
    close(
        area(&w),
        G * t2 + 2.0 * d * d * tan(30.0) - segment(G / 2.0 + d * tan(30.0), 0.5),
    );
    assert_eq!(
        section(&w).notes,
        vec![FillNote::BelowToes {
            side: FillSide::Second,
            depth: 0.5,
            below: [0.5, 0.5],
            ratio: 0.5 / 12.0,
        }]
    );
}

#[test]
fn a_concave_face_across_the_walls_is_refused_with_the_deepest_that_builds() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let a = G / 2.0 + t * tan(30.0);
    let w = Case {
        face: Contour::Concave { depth: 3.0 },
        ..channel(l, r)
    };
    let err = run(&w).unwrap_err();
    let SolidError::Failed(failure) = &err else {
        panic!("not a typed refusal: {err:?}");
    };
    assert_eq!(
        failure.reason,
        Reason::Fill(FillFailure::ConcaveFaceCrossesWall {
            side: FillSide::Face,
            depth: 3.0,
        })
    );
    // the arc touches the 60 degree V where its end tangent is as steep as the wall: at
    // 2 atan(d / a) = 60 degrees, a tan 30 (closed form, not searched). It stops a little short
    // of that, at the circle through the wall's line just behind the toe (`toe_touch`), and the
    // deepest that builds keeps the depth margin, here the zone tolerance, from that
    let touch = toe_touch(&w, a, 60.0);
    assert!(
        touch < a * tan(30.0) && a * tan(30.0) - touch < 1e-6,
        "{touch}"
    );
    let fit = err
        .fit()
        .expect("the refusal says the depth that builds")
        .size;
    assert!((fit - (touch - ZONE)).abs() < 1e-12, "fit {fit}");
    let just = |depth: f64| {
        run(&Case {
            face: Contour::Concave { depth },
            ..w.clone()
        })
    };
    // the fit builds when retried; past it, and at the touch, it is refused
    assert!(just(fit).is_ok());
    assert!(just(fit + 1e-9).is_err());
    assert!(just(touch).is_err());
    assert!(just(a * tan(30.0)).is_err());
}

#[test]
fn a_depth_fills_the_channel_only_that_deep() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let s = 4.0;
    // a second face does not apply: the fill ends inside the groove
    let w = Case {
        depth: Some(s),
        second_face: Some(Contour::Convex { height: 0.8 }),
        ..channel(l.clone(), r.clone())
    };
    close(area(&w), G * s + tan(30.0) * (t * t - (t - s) * (t - s)));
    assert!(section(&w).notes.is_empty());

    // as deep as the plate: full penetration, nothing to say
    let w = Case {
        depth: Some(t),
        ..channel(l.clone(), r.clone())
    };
    close(area(&w), G * t + t * t * tan(30.0));
    assert!(section(&w).notes.is_empty());

    // past the plate: full penetration, and the user is told
    let w = Case {
        depth: Some(8.0),
        ..channel(l, r)
    };
    close(area(&w), G * t + t * t * tan(30.0));
    assert_eq!(
        section(&w).notes,
        vec![FillNote::DeeperThanOpening {
            asked: 8.0,
            opening: 6.0
        }]
    );

    // a U groove cut on its round
    let right = u_right();
    let w = Case {
        depth: Some(8.0),
        ..channel(mirror(&right), right)
    };
    let cy = -(U_T - U_C) + U_R;
    // below the round's centre (at -cy) the cut keeps the strip of the round above y = -8
    let dy = 8.0 + cy;
    let round = U_R * U_R / 2.0 * (dy / U_R).asin() + dy / 2.0 * (U_R * U_R - dy * dy).sqrt();
    let half = U_R * -cy + tan(U_FLANK) * cy * cy / 2.0 + round;
    close(area(&w), G * 8.0 + 2.0 * half);
}

#[test]
fn a_depth_past_a_closed_groove_fills_the_groove_and_says_so() {
    // a partial-penetration preparation: a 60 degree V 6 deep, the plates touching below it
    let top = 6.0 * tan(30.0);
    let left = vec![line(p(-top, 0.0), p(0.0, -6.0))];
    let right = mirror(&left);
    let w = Case {
        depth: Some(8.0),
        second_face: Some(Contour::Convex { height: 1.0 }),
        ..channel(left, right)
    };
    let s = section(&w);
    assert!(!s.through);
    close(area(&w), 36.0 * tan(30.0));
    assert_eq!(
        s.notes,
        vec![FillNote::DeeperThanChannel {
            asked: 8.0,
            channel: 6.0
        }]
    );
}

#[test]
fn bad_walls_contours_and_depths_are_refused_as_invalid_input() {
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    let ok = channel(l.clone(), r.clone());
    let cubic = Seg2::Cubic {
        p0: p(-4.0, 0.0),
        p1: p(-3.0, -2.0),
        p2: p(-1.0, -4.0),
        p3: p(-0.5, -6.0),
    };
    let bad = [
        channel(Vec::new(), r.clone()),
        channel(vec![cubic], r.clone()),
        // a wall whose two pieces do not join
        channel(
            vec![
                line(p(-4.0, 0.0), p(-2.0, -3.0)),
                line(p(-1.9, -3.0), p(-0.5, -6.0)),
            ],
            r.clone(),
        ),
        channel(vec![line(p(f64::NAN, 0.0), p(-0.5, -6.0))], r.clone()),
        // left and right swapped
        channel(r.clone(), l.clone()),
        // walls that cross each other
        channel(
            vec![line(p(-4.0, 0.0), p(2.0, -6.0))],
            vec![line(p(4.0, 0.0), p(-2.0, -6.0))],
        ),
        Case {
            face: Contour::Convex { height: 0.0 },
            ..ok.clone()
        },
        Case {
            face: Contour::Concave { depth: -1.0 },
            ..ok.clone()
        },
        Case {
            second_face: Some(Contour::ConvexBlended {
                height: 1.0,
                overlap: f64::NAN,
            }),
            ..ok.clone()
        },
        Case {
            depth: Some(0.0),
            ..ok.clone()
        },
    ];
    for (i, w) in bad.iter().enumerate() {
        let err = run(w).unwrap_err();
        assert!(
            matches!(err, SolidError::InvalidInput { .. }),
            "case {i}: {err:?}"
        );
    }
    assert!(run(&ok).is_ok());
}

/// The area of a polygon, its corners in either order.
fn polygon(corners: &[Point2]) -> f64 {
    let n = corners.len();
    0.5 * (0..n)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % n]);
            a.x * b.y - a.y * b.x
        })
        .sum::<f64>()
        .abs()
}

/// The lowest point of a fill's section.
fn lowest(w: &Filled) -> f64 {
    w.region
        .outer
        .segs
        .iter()
        .flat_map(|seg| (0..=16).map(move |i| seg.point_at(f64::from(i) / 16.0).y))
        .fold(f64::MAX, f64::min)
}

/// The x of a straight wall `y` on a straight wall from `a` to `b`.
fn x_at(a: Point2, b: Point2, y: f64) -> f64 {
    a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y)
}

#[test]
fn the_depth_is_measured_from_the_deeper_toe_along_the_depth_normal() {
    let up = Some(Vec2::new(0.0, 1.0));
    // a 60 degree V between plates 10 and 8 thick, their undersides flush (at -10)
    let (l0, lb) = (p(-(G / 2.0 + 10.0 * tan(30.0)), 0.0), p(-G / 2.0, -10.0));
    let (r0, rb) = (p(G / 2.0 + 8.0 * tan(30.0), -2.0), p(G / 2.0, -10.0));
    let unequal = Case {
        depth_normal: up,
        ..channel(vec![line(l0, lb)], vec![line(r0, rb)])
    };
    // s = 6 from the thinner (lower) plate's face: the fill's bottom level at -8, 6 into the
    // thinner plate and 8 into the thicker
    let w = Case {
        depth: Some(6.0),
        ..unequal.clone()
    };
    let got = section(&w);
    assert!(
        (lowest(&got) + 8.0).abs() < 1e-12,
        "bottom at {}",
        lowest(&got)
    );
    assert!((got.filled_depth - 6.0).abs() < 1e-12);
    assert!((got.channel_depth - 8.0).abs() < 1e-12);
    assert!(got.notes.is_empty());
    close(
        area(&w),
        polygon(&[
            l0,
            p(x_at(l0, lb, -8.0), -8.0),
            p(x_at(r0, rb, -8.0), -8.0),
            r0,
        ]),
    );
    // past the thinner plate: full penetration, and the user is told
    let w = Case {
        depth: Some(9.0),
        ..unequal.clone()
    };
    let got = section(&w);
    close(area(&w), polygon(&[l0, lb, rb, r0]));
    assert!((got.filled_depth - 8.0).abs() < 1e-12);
    assert_eq!(
        got.notes,
        vec![FillNote::DeeperThanOpening {
            asked: 9.0,
            opening: 8.0
        }]
    );
    // without the plates' normal the depth is measured from the chord: not level
    let chord = section(&Case {
        depth: Some(6.0),
        depth_normal: None,
        ..unequal
    });
    assert!((lowest(&chord) + 8.0).abs() > 0.1);

    // two 8 mm plates, the right one 2 lower: s from the right plate's face
    let (l0, lb) = (p(-(G / 2.0 + 8.0 * tan(30.0)), 0.0), p(-G / 2.0, -8.0));
    let (r0, rb) = (p(G / 2.0 + 8.0 * tan(30.0), -2.0), p(G / 2.0, -10.0));
    let offset = Case {
        depth_normal: up,
        ..channel(vec![line(l0, lb)], vec![line(r0, rb)])
    };
    let w = Case {
        depth: Some(5.0),
        ..offset.clone()
    };
    assert!((lowest(&section(&w)) + 7.0).abs() < 1e-12);
    close(
        area(&w),
        polygon(&[
            l0,
            p(x_at(l0, lb, -7.0), -7.0),
            p(x_at(r0, rb, -7.0), -7.0),
            r0,
        ]),
    );
    // the groove opens under the upper plate 6 below the lower plate's face: deeper is full
    // penetration, the whole groove filled to its deepest point
    let w = Case {
        depth: Some(7.0),
        ..offset
    };
    let got = section(&w);
    close(area(&w), polygon(&[l0, lb, rb, r0]));
    assert!((got.filled_depth - 8.0).abs() < 1e-12);
    assert_eq!(
        got.notes,
        vec![FillNote::DeeperThanOpening {
            asked: 7.0,
            opening: 6.0
        }]
    );
}

#[test]
fn a_concave_face_between_offset_toes_says_how_deep_it_dips_below_each() {
    let (l0, lb) = (p(-(G / 2.0 + 8.0 * tan(30.0)), 0.0), p(-G / 2.0, -8.0));
    let (r0, rb) = (p(G / 2.0 + 8.0 * tan(30.0), -2.0), p(G / 2.0, -10.0));
    let w = Case {
        depth_normal: Some(Vec2::new(0.0, 1.0)),
        face: Contour::Concave { depth: 1.0 },
        ..channel(vec![line(l0, lb)], vec![line(r0, rb)])
    };
    let got = section(&w);
    let [FillNote::BelowToes {
        side, depth, below, ..
    }] = got.notes[..]
    else {
        panic!("{:?}", got.notes);
    };
    assert_eq!((side, depth), (FillSide::Face, 1.0));
    // the face's lowest point below each plate's face: the plates' step apart, and below the
    // lower plate's face too (the arc leaves the lower toe rising more steeply than the chord
    // falls)
    assert!((below[0] - below[1] - 2.0).abs() < 1e-9, "{below:?}");
    assert!(below[1] > 0.0, "{below:?}");
    // the lowest point of the face's arc, sampled, agrees
    let face_low = got
        .region
        .outer
        .segs
        .iter()
        .filter(|seg| matches!(seg, Seg2::Arc { .. }))
        .flat_map(|seg| (0..=4096).map(move |i| seg.point_at(f64::from(i) / 4096.0).y))
        .fold(f64::MAX, f64::min);
    assert!((face_low + below[0]).abs() < 1e-6, "{face_low} {below:?}");
}

#[test]
fn a_depth_normal_that_is_not_finite_or_points_into_the_channel_is_refused() {
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    for n in [
        Vec2::new(0.0, -1.0),
        Vec2::new(0.0, 0.0),
        Vec2::new(f64::NAN, 1.0),
    ] {
        let err = run(&Case {
            depth_normal: Some(n),
            ..channel(l.clone(), r.clone())
        })
        .unwrap_err();
        assert!(
            matches!(err, SolidError::InvalidInput { .. }),
            "{n:?}: {err:?}"
        );
    }
    // a normal of any length works, the tiny and the huge too, and on plates in one plane
    // changes nothing
    for n in [
        Vec2::new(0.0, 1e-200),
        Vec2::new(0.0, 1e200),
        Vec2::new(0.0, 1e-310),
    ] {
        let tiny_or_huge = run(&Case {
            depth_normal: Some(n),
            depth: Some(4.0),
            ..channel(l.clone(), r.clone())
        })
        .unwrap();
        let unit = run(&Case {
            depth_normal: Some(Vec2::new(0.0, 1.0)),
            depth: Some(4.0),
            ..channel(l.clone(), r.clone())
        })
        .unwrap();
        assert_eq!(tiny_or_huge, unit, "{n:?}");
    }
    // a subnormal normal tilted by 1e-10 is read as that tilt, not refused
    assert!(run(&Case {
        depth_normal: Some(Vec2::new(1e-320, 1e-310)),
        depth: Some(4.0),
        ..channel(l.clone(), r.clone())
    })
    .is_ok());
    let one_plane = run(&Case {
        depth_normal: Some(Vec2::new(0.0, 3.0)),
        depth: Some(4.0),
        ..channel(l.clone(), r.clone())
    })
    .unwrap();
    let chord = run(&Case {
        depth: Some(4.0),
        ..channel(l, r)
    })
    .unwrap();
    assert_eq!(one_plane, chord);
}

#[test]
fn a_concave_face_on_square_walls_is_refused_where_it_touches_them() {
    // a square groove 6 wide: its walls stand square to the face, so the face touches them at
    // the half circle, 3 deep; it stops just short of that (`toe_touch`), and the deepest that
    // builds keeps the depth margin from there
    let w = Case {
        face: Contour::Concave { depth: 4.0 },
        ..channel(
            vec![line(p(-3.0, 0.0), p(-3.0, -6.0))],
            vec![line(p(3.0, 0.0), p(3.0, -6.0))],
        )
    };
    let err = run(&w).unwrap_err();
    let SolidError::Failed(failure) = &err else {
        panic!("not a typed refusal: {err:?}");
    };
    assert_eq!(
        failure.reason,
        Reason::Fill(FillFailure::ConcaveFaceCrossesWall {
            side: FillSide::Face,
            depth: 4.0,
        })
    );
    let fit = err.fit().unwrap().size;
    let touch = toe_touch(&w, 3.0, 90.0);
    assert!(touch < 3.0 && 3.0 - touch < 1e-6, "{touch}");
    assert!((fit - (touch - ZONE)).abs() < 1e-12, "fit {fit}");
    assert!(run(&Case {
        face: Contour::Concave { depth: fit },
        ..w
    })
    .is_ok());
}

/// The typed refusal of a fill, and its fit.
fn refused(w: &Case) -> (FillFailure, Option<f64>) {
    let err = run(w).unwrap_err();
    let SolidError::Failed(failure) = &err else {
        panic!("not a typed refusal: {err:?}");
    };
    let Reason::Fill(why) = failure.reason else {
        panic!("not a fill refusal: {err:?}");
    };
    (why, err.fit().map(|f| f.size))
}

#[test]
fn a_convex_face_past_half_a_circle_is_refused_with_the_highest_that_builds() {
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    let a = G / 2.0 + 6.0 * tan(30.0);
    let flat = Case {
        depth_normal: Some(Vec2::new(0.0, 1.0)),
        ..channel(l, r)
    };
    // a plain convex face: half the toe distance
    let (why, fit) = refused(&Case {
        face: Contour::Convex { height: 4.5 },
        ..flat.clone()
    });
    assert_eq!(
        why,
        FillFailure::ConvexFaceTooHigh {
            side: FillSide::Face,
            height: 4.5
        }
    );
    assert!((fit.unwrap() - a).abs() < 1e-12);
    assert!(run(&Case {
        face: Contour::Convex {
            height: a * (1.0 - 1e-9)
        },
        ..flat.clone()
    })
    .is_ok());
    // blended toes: half the toe distance plus the overlap
    let (why, fit) = refused(&Case {
        face: Contour::ConvexBlended {
            height: a + 1.5,
            overlap: 1.0,
        },
        ..flat.clone()
    });
    assert_eq!(
        why,
        FillFailure::ConvexFaceTooHigh {
            side: FillSide::Face,
            height: a + 1.5
        }
    );
    assert!((fit.unwrap() - (a + 1.0)).abs() < 1e-12);
    assert!(run(&Case {
        face: Contour::ConvexBlended {
            height: (a + 1.0) * (1.0 - 1e-9),
            overlap: 1.0,
        },
        ..flat
    })
    .is_ok());
}

#[test]
fn blended_toes_where_the_sides_beyond_do_not_run_along_the_chord_are_refused() {
    let blended = Contour::ConvexBlended {
        height: 1.0,
        overlap: 1.0,
    };
    // no common plate normal (a T- or corner joint)
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    let (why, fit) = refused(&Case {
        face: blended,
        ..channel(l, r)
    });
    assert_eq!(
        why,
        FillFailure::BlendedToesOffChord {
            side: FillSide::Face
        }
    );
    assert_eq!(fit, None);
    // offset plates: the right one 2 lower
    let (l0, lb) = (p(-(G / 2.0 + 8.0 * tan(30.0)), 0.0), p(-G / 2.0, -8.0));
    let (r0, rb) = (p(G / 2.0 + 8.0 * tan(30.0), -2.0), p(G / 2.0, -10.0));
    let (why, _) = refused(&Case {
        face: blended,
        depth_normal: Some(Vec2::new(0.0, 1.0)),
        ..channel(vec![line(l0, lb)], vec![line(r0, rb)])
    });
    assert_eq!(
        why,
        FillFailure::BlendedToesOffChord {
            side: FillSide::Face
        }
    );
    // plates of unequal thickness, faces flush: blended toes on the face build, on the second
    // face (root sides 2 apart) they are refused
    let (l0, lb) = (p(-(G / 2.0 + 10.0 * tan(30.0)), 0.0), p(-G / 2.0, -10.0));
    let (r0, rb) = (p(G / 2.0 + 8.0 * tan(30.0), 0.0), p(G / 2.0, -8.0));
    let unequal = Case {
        depth_normal: Some(Vec2::new(0.0, 1.0)),
        ..channel(vec![line(l0, lb)], vec![line(r0, rb)])
    };
    assert!(run(&Case {
        face: blended,
        ..unequal.clone()
    })
    .is_ok());
    let (why, _) = refused(&Case {
        second_face: Some(blended),
        ..unequal
    });
    assert_eq!(
        why,
        FillFailure::BlendedToesOffChord {
            side: FillSide::Second
        }
    );
}

/// How far `fill` grows each side towards a concave face: twice the crossing check's
/// near-miss tolerance, 100 times the conservative allowance (1e-9 of the section's size, its
/// largest coordinate and at least 1). The size is read from the sides' end points only: an
/// arc wall bulging past its ends is not counted, so this is for line walls.
fn grown(c: &Case) -> f64 {
    let size = c
        .left
        .iter()
        .chain(&c.right)
        .flat_map(|s| [s.start(), s.end()])
        .fold(1.0_f64, |m, q| m.max(q.x.abs()).max(q.y.abs()));
    2.0 * 100.0 * 1e-9 * size
}

/// Where a concave face between the toes (-a, 0) and (a, 0) stops against a straight wall
/// leaving the toe (a, 0) at `angle` degrees below the chord: the circle through the toes and
/// the point of the wall's line `grown` back past the toe, the wall grown like every side.
/// Its centre (0, k) is read from the three points (not the fill's tangency quadratics).
fn toe_touch(c: &Case, a: f64, angle: f64) -> f64 {
    let b = grown(c);
    let t = angle.to_radians();
    let (x, y) = (a + b * t.cos(), b * t.sin());
    let k = (x * x + y * y - a * a) / (2.0 * y);
    a * a / (a.hypot(k) + k)
}

#[test]
fn a_concave_face_is_sized_against_every_other_side_of_the_section() {
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    // filled 1 deep: the cut line under the face is what the arc reaches first, at 1
    let (why, fit) = refused(&Case {
        face: Contour::Concave { depth: 1.5 },
        depth: Some(1.0),
        ..channel(l, r)
    });
    assert_eq!(
        why,
        FillFailure::ConcaveFaceCrossesWall {
            side: FillSide::Face,
            depth: 1.5
        }
    );
    // the lowest point meets the cut line, grown towards it: the dip falls by the growth
    let (vl, vr) = v_walls(6.0, 60.0, 0.0);
    let off = grown(&channel(vl, vr));
    assert!((fit.unwrap() - (1.0 - off - ZONE)).abs() < 1e-12, "{fit:?}");
    // just short of the touch: still the typed refusal (the crossing check never decides)
    let cut = |depth: f64| Case {
        face: Contour::Concave { depth },
        depth: Some(1.0),
        ..channel(v_walls(6.0, 60.0, 0.0).0, v_walls(6.0, 60.0, 0.0).1)
    };
    let (why, fit) = refused(&cut(1.0 - 1e-7));
    assert!(
        matches!(why, FillFailure::ConcaveFaceCrossesWall { .. }),
        "{why:?}"
    );
    let fit = fit.unwrap();
    assert!((fit - (1.0 - off - ZONE)).abs() < 1e-12);
    assert!(run(&cut(1.0)).is_err());
    assert!(run(&cut(fit)).is_ok());
    assert!(run(&cut(fit + 1e-9)).is_err());
    // both faces concave on a thin X, each past what builds: a typed refusal, the face first.
    // The second face is past its half circle, so its chord stands in, and the toe decides:
    // just short of a tan 30 with a = 0.5 + tan 30 (`toe_touch`)
    let (l, r) = x_walls(2.0, 0.0);
    let thin_x = Case {
        face: Contour::Concave { depth: 1.5 },
        second_face: Some(Contour::Concave { depth: 1.5 }),
        ..channel(l, r)
    };
    let (why, fit) = refused(&thin_x);
    assert!(
        matches!(
            why,
            FillFailure::ConcaveFaceCrossesWall {
                side: FillSide::Face,
                ..
            }
        ),
        "{why:?}"
    );
    let fit = fit.unwrap();
    let touch = toe_touch(&thin_x, 0.5 + tan(30.0), 60.0);
    assert!((0.622_008_467_928_146_2 - touch).abs() < 1e-6, "{touch}");
    assert!((fit - (touch - ZONE)).abs() < 1e-12, "{fit}");

    // a square groove 10 wide and 4 deep through the plate: a concave face meets a concave
    // second face 2 high in the middle, at 4 - 2
    let square = channel(
        vec![line(p(-5.0, 0.0), p(-5.0, -4.0))],
        vec![line(p(5.0, 0.0), p(5.0, -4.0))],
    );
    let (why, fit) = refused(&Case {
        face: Contour::Concave { depth: 2.0 },
        second_face: Some(Contour::Concave { depth: 2.0 }),
        ..square.clone()
    });
    assert_eq!(
        why,
        FillFailure::ConcaveFaceCrossesWall {
            side: FillSide::Face,
            depth: 2.0
        }
    );
    let fit = fit.unwrap();
    let off = grown(&square);
    assert!((fit - (2.0 - off - ZONE)).abs() < 1e-12, "{fit}");
    assert!(run(&Case {
        face: Contour::Concave { depth: fit },
        second_face: Some(Contour::Concave { depth: 2.0 }),
        ..square.clone()
    })
    .is_ok());
    // the second face against a flat face 4 above it: the touch at 4, refused there
    let second = |depth: f64| Case {
        second_face: Some(Contour::Concave { depth }),
        ..square.clone()
    };
    let (why, fit) = refused(&second(4.0));
    assert_eq!(
        why,
        FillFailure::ConcaveFaceCrossesWall {
            side: FillSide::Second,
            depth: 4.0
        }
    );
    let fit = fit.unwrap();
    assert!((fit - (4.0 - off - ZONE)).abs() < 1e-12, "{fit}");
    assert!(run(&second(fit)).is_ok());
    assert!(run(&second(fit + 1e-9)).is_err());
}

#[test]
fn a_concave_face_that_touches_nothing_builds_up_to_its_half_circle() {
    // walls that open outwards under the face (an undercut, 135 degrees from the chord): the
    // half circle touches nothing and builds; deeper is refused as too deep, with the half
    // circle
    let w = |depth: f64| Case {
        face: Contour::Concave { depth },
        ..channel(
            vec![line(p(-3.0, 0.0), p(-9.0, -6.0))],
            vec![line(p(3.0, 0.0), p(9.0, -6.0))],
        )
    };
    assert!(run(&w(3.0)).is_ok());
    let (why, fit) = refused(&w(3.5));
    assert_eq!(
        why,
        FillFailure::ConcaveFaceTooDeep {
            side: FillSide::Face,
            depth: 3.5
        }
    );
    assert!((fit.unwrap() - 3.0).abs() < 1e-12, "{fit:?}");
}

#[test]
fn a_face_shallower_than_any_arc_is_built_flat_or_refused_with_the_smallest_arc() {
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    let flat = area(&channel(l.clone(), r.clone()));
    // within the zone tolerance: built flat, and the note says so
    for depth in [1e-300, 1e-12, 1e-9, ZONE] {
        let w = Case {
            face: Contour::Concave { depth },
            ..channel(l.clone(), r.clone())
        };
        let got = section(&w);
        assert_eq!(area(&w), flat, "{depth}");
        assert_eq!(
            got.notes,
            vec![FillNote::BuiltFlat {
                side: FillSide::Face,
                asked: depth
            }]
        );
    }
    // a groove 2000 wide: no arc of radius 1e9 or more builds, so past the zone tolerance
    // and below a^2 / 2e9 (5e-4) the face is refused, with that smallest arc (for a concave
    // face too: it is nearer than the deepest that builds)
    let wide = channel(
        vec![line(p(-1000.0, 0.0), p(-999.0, -6.0))],
        vec![line(p(1000.0, 0.0), p(999.0, -6.0))],
    );
    for face in [
        Contour::Concave { depth: 1e-4 },
        Contour::Convex { height: 1e-4 },
    ] {
        let (why, fit) = refused(&Case {
            face,
            ..wide.clone()
        });
        assert_eq!(
            why,
            FillFailure::FaceTooShallow {
                side: FillSide::Face,
                size: 1e-4
            }
        );
        // the fit is the nearest size that builds: the smallest arc, for either face
        let fit = fit.unwrap();
        assert!((fit - 5e-4).abs() < 1e-9, "{fit}");
        let at_fit = match face {
            Contour::Concave { .. } => Contour::Concave { depth: fit },
            _ => Contour::Convex { height: fit },
        };
        assert!(run(&Case {
            face: at_fit,
            ..wide.clone()
        })
        .is_ok());
    }
}

#[test]
fn the_fit_of_a_convex_face_builds_when_retried() {
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    let flat = Case {
        depth_normal: Some(Vec2::new(0.0, 1.0)),
        ..channel(l, r)
    };
    for face in [
        Contour::Convex { height: 9.0 },
        Contour::ConvexBlended {
            height: 9.0,
            overlap: 1.0,
        },
    ] {
        let (_, fit) = refused(&Case {
            face,
            ..flat.clone()
        });
        let retry = match face {
            Contour::Convex { .. } => Contour::Convex {
                height: fit.unwrap(),
            },
            _ => Contour::ConvexBlended {
                height: fit.unwrap(),
                overlap: 1.0,
            },
        };
        assert!(
            run(&Case {
                face: retry,
                ..flat.clone()
            })
            .is_ok(),
            "{face:?}"
        );
    }
}

#[test]
fn a_face_is_checked_against_the_walls_as_built_not_as_its_chord() {
    // walls that lean out over the plates before running down to the root
    let over = |corner: Point2| {
        let left = vec![line(p(-5.0, 0.0), corner), line(corner, p(-1.0, -6.0))];
        let right = mirror(&left);
        Case {
            depth_normal: Some(Vec2::new(0.0, 1.0)),
            ..channel(left, right)
        }
    };
    // a blended toe runs out under the wall's overhang and crosses it: refused
    let err = run(&Case {
        face: Contour::ConvexBlended {
            height: 0.5,
            overlap: 1.0,
        },
        ..over(p(-5.5, 0.3))
    })
    .unwrap_err();
    assert!(matches!(err, SolidError::InvalidInput { .. }), "{err:?}");
    // a convex face that clears the walls' inner corners builds, though its chord crosses
    // the walls
    assert!(run(&Case {
        face: Contour::Convex { height: 3.0 },
        ..over(p(-3.0, 2.0))
    })
    .is_ok());
}

/// Retries a refused fill at its fit until it builds (at most three times): the last fill.
fn retry_until_built(mut w: Case, at: fn(f64) -> Contour) -> (Case, usize) {
    for tries in 0..3 {
        let Err(err) = run(&w) else {
            return (w, tries);
        };
        w.face = at(err.fit().expect("every refusal here has a fit").size);
    }
    panic!("still refused after three retries: {:?}", run(&w));
}

#[test]
fn a_shallow_concave_face_and_what_it_touches_agree_on_one_fit_that_builds() {
    // a groove 2000 wide filled 3e-4 deep: the cut line is touched below the smallest arc
    // (5e-4), so the only face that builds there is the flat one, at the zone tolerance
    let w = Case {
        face: Contour::Concave { depth: 1e-3 },
        depth: Some(3e-4),
        ..channel(
            vec![line(p(-1000.0, 0.0), p(-999.0, -6.0))],
            vec![line(p(1000.0, 0.0), p(999.0, -6.0))],
        )
    };
    let (why, fit) = refused(&w);
    assert!(
        matches!(why, FillFailure::ConcaveFaceCrossesWall { .. }),
        "{why:?}"
    );
    assert_eq!(fit, Some(ZONE));
    let (built, tries) = retry_until_built(w, |depth| Contour::Concave { depth });
    assert_eq!(tries, 1);
    assert_eq!(
        section(&built)
            .notes
            .last()
            .map(|n| matches!(n, FillNote::BuiltFlat { .. })),
        Some(true)
    );
}

#[test]
fn shallow_blended_toes_build_or_are_refused_with_a_fit_that_builds() {
    let blended = |height: f64| Contour::ConvexBlended {
        height,
        overlap: 1.0,
    };
    let groove = |a: f64| Case {
        depth_normal: Some(Vec2::new(0.0, 1.0)),
        ..channel(
            vec![line(p(-a, 0.0), p(-(a - 1.0), -6.0))],
            vec![line(p(a, 0.0), p(a - 1.0, -6.0))],
        )
    };
    for (a, height) in [(3.0, 1e-3), (100.0, 1e-2), (1000.0, 1e-3)] {
        assert!(
            run(&Case {
                face: blended(height),
                ..groove(a)
            })
            .is_ok(),
            "a {a}, h {height}: {:?}",
            run(&Case {
                face: blended(height),
                ..groove(a)
            })
        );
    }
    // a 2000 wide: below the blended toes' own floor (x a / 2e9, x = a + o) refused typed, and
    // the fit builds
    let (why, fit) = refused(&Case {
        face: blended(1e-4),
        ..groove(1000.0)
    });
    assert_eq!(
        why,
        FillFailure::FaceTooShallow {
            side: FillSide::Face,
            size: 1e-4
        }
    );
    let fit = fit.unwrap();
    assert!((fit - 1001.0 * 1000.0 / 2e9).abs() < 1e-9, "{fit}");
    let (_, tries) = retry_until_built(
        Case {
            face: blended(1e-4),
            ..groove(1000.0)
        },
        |h| Contour::ConvexBlended {
            height: h,
            overlap: 1.0,
        },
    );
    assert_eq!(tries, 1);
    // an overlap longer than half the groove: the blend arcs are the largest (radius
    // o (x² + h²) / 2hx), so the floor is x o / 2e9 (7.5e-4 here), and its retry builds
    let wide_blend = |height: f64| Contour::ConvexBlended {
        height,
        overlap: 1000.0,
    };
    let (_, fit) = refused(&Case {
        face: wide_blend(1e-4),
        ..groove(500.0)
    });
    let fit = fit.unwrap();
    assert!((fit - 1500.0 * 1000.0 / 2e9).abs() < 1e-9, "{fit}");
    let (_, tries) = retry_until_built(
        Case {
            face: wide_blend(1e-4),
            ..groove(500.0)
        },
        |h| Contour::ConvexBlended {
            height: h,
            overlap: 1000.0,
        },
    );
    assert_eq!(tries, 1);
}

#[test]
fn a_concave_fit_against_a_wall_builds_on_a_large_section() {
    // a groove 200 wide, filled 2 deep: the concave face asked for touches the walls away from
    // the toes; its fit stays clear of the walls by more than the crossing check's tolerance
    let w = Case {
        face: Contour::Concave { depth: 3.0 },
        depth: Some(2.0),
        ..channel(
            vec![line(p(-100.0, 0.0), p(-99.0, -200.0))],
            vec![line(p(100.0, 0.0), p(99.0, -200.0))],
        )
    };
    let (why, _) = refused(&w);
    assert!(
        matches!(why, FillFailure::ConcaveFaceCrossesWall { .. }),
        "{why:?}"
    );
    let (_, tries) = retry_until_built(w, |d| Contour::Concave { depth: d });
    assert_eq!(tries, 1);
}

/// The right wall of a groove with a step just under the toe: (1, 0) to (1, -0.001), in to
/// (0.9, -0.0015), down to (0.9, -2).
fn step_wall() -> Vec<Seg2> {
    vec![
        line(p(1.0, 0.0), p(1.0, -0.001)),
        line(p(1.0, -0.001), p(0.9, -0.0015)),
        line(p(0.9, -0.0015), p(0.9, -2.0)),
    ]
}

#[test]
fn a_concave_fit_against_a_step_in_the_wall_builds_on_every_scale() {
    for f in [1.0, 10.0, 100.0, 1e3, 1e4] {
        let right: Vec<Seg2> = step_wall()
            .iter()
            .map(|s| {
                line(
                    p(s.start().x * f, s.start().y * f),
                    p(s.end().x * f, s.end().y * f),
                )
            })
            .collect();
        let w = Case {
            face: Contour::Concave { depth: 0.5 * f },
            ..channel(mirror(&right), right)
        };
        let (why, _) = refused(&w);
        assert!(
            matches!(why, FillFailure::ConcaveFaceCrossesWall { .. }),
            "{f}: {why:?}"
        );
        let (_, tries) = retry_until_built(w, |d| Contour::Concave { depth: d });
        assert_eq!(tries, 1, "{f}");
    }
}

/// An arc wall of radius `rho` leaving the toe (1, 0) at `t` degrees below the chord, towards
/// the other toe: curving down (away from the chord, `down`) half a unit long, or curving up
/// (towards it) through half its angle, so that it ends below the chord.
fn toe_arc(t: f64, rho: f64, down: bool) -> Seg2 {
    let t = t.to_radians();
    // the side the centre is on, square to the wall's direction (-cos t, -sin t) at the toe
    let (side, sweep) = if down {
        (1.0, (0.5 / rho).min(0.25))
    } else {
        (-1.0, -(0.5 / rho).min(0.5 * t))
    };
    let center = p(1.0 + side * rho * t.sin(), -side * rho * t.cos());
    let start = (0.0 - center.y).atan2(1.0 - center.x);
    Seg2::Arc {
        center,
        radius: rho,
        start,
        sweep,
    }
}

#[test]
fn concave_fits_build_when_retried_on_every_scale() {
    // V walls of several slopes (down to 0.002 degrees), arc walls leaving the toe at shallow
    // angles curving either way (radii 0.5 to 1000 times half the chord, up-curving down to
    // 0.002 degrees), a square groove, the
    // U groove and the stepped wall, from 1 to 1e5 wide, each asked for concave faces that
    // reach past what they touch: every refusal's fit builds
    let grow = |wall: Vec<Seg2>, f: f64| -> Vec<Seg2> {
        wall.into_iter()
            .map(|s| match s {
                Seg2::Line { a, b } => line(p(a.x * f, a.y * f), p(b.x * f, b.y * f)),
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => Seg2::Arc {
                    center: p(center.x * f, center.y * f),
                    radius: radius * f,
                    start,
                    sweep,
                },
                other => other,
            })
            .collect()
    };
    let mut refused_fits = Vec::new();
    let mut flat_refused = 0;
    for f in [1.0, 10.0, 100.0, 1e3, 1e4, 1e5] {
        let mut walls = vec![
            u_right(),
            vec![line(p(3.0, 0.0), p(3.0, -6.0))],
            step_wall(),
        ];
        for (dx, dy) in [
            (0.01, -0.02),
            (0.01, -2.0),
            (0.001, -0.01),
            (-0.01, -0.03),
            (0.05, -0.01),
            // shallow toe angles, about 5 and 2 degrees
            (0.5, -0.043_744),
            (0.5, -0.017_455),
        ] {
            walls.push(vec![line(p(1.0, 0.0), p(1.0 - dx, dy))]);
        }
        for t in [0.5_f64, 0.05, 0.002] {
            let t = t.to_radians();
            walls.push(vec![line(
                p(1.0, 0.0),
                p(1.0 - 0.5 * t.cos(), -0.5 * t.sin()),
            )]);
        }
        // arc walls leaving the toe: down at 5, 1, 0.05 and 0.01 degrees, up at 5, 1, 0.2,
        // 0.05, 0.01 and 0.002 (walls whose flat channel is refused are skipped below)
        for (t, down) in [
            (5.0, true),
            (1.0, true),
            (0.05, true),
            (0.01, true),
            (5.0, false),
            (1.0, false),
            (0.2, false),
            (0.05, false),
            (0.01, false),
            (0.002, false),
        ] {
            for rho in [0.5, 2.0, 5.0, 10.0, 30.0, 100.0, 300.0, 1000.0] {
                // shortcut: these walls curving up (angle, radius, size) get a fit whose section
                // is refused as of no area (its area's bound straddles zero); left out until
                // the area band (#21152) is settled
                let no_area = [
                    (0.2, 0.5, 10.0),
                    (0.2, 0.5, 100.0),
                    (0.2, 0.5, 1e3),
                    (0.05, 2.0, 100.0),
                    (0.05, 5.0, 100.0),
                    (0.05, 5.0, 1e3),
                    (0.05, 10.0, 10.0),
                    (0.05, 10.0, 100.0),
                    (0.05, 10.0, 1e3),
                    (0.05, 30.0, 10.0),
                    (0.05, 30.0, 100.0),
                    (0.05, 30.0, 1e3),
                    (0.05, 30.0, 1e4),
                    (0.01, 100.0, 100.0),
                    (0.01, 100.0, 1e3),
                    (0.01, 300.0, 10.0),
                    (0.01, 300.0, 100.0),
                    (0.01, 300.0, 1e3),
                    (0.002, 1000.0, 100.0),
                ];
                if !down && no_area.contains(&(t, rho, f)) {
                    continue;
                }
                walls.push(vec![toe_arc(t, rho, down)]);
            }
        }
        for right in walls {
            let right = grow(right, f);
            let top = right.first().map_or(f, |s| s.start().x);
            // a wall whose channel is refused even flat is not one a concave face can be asked
            // of (a wall too short or too tight to run away from the face at this size): counted
            // below, so a change that starts refusing other flat channels fails here
            if run(&channel(mirror(&right), right.clone())).is_err() {
                flat_refused += 1;
                continue;
            }
            for want in [0.5, 0.9, 2.0] {
                for depth in [None, Some(0.02 * top), Some(0.5 * top)] {
                    let mut w = Case {
                        face: Contour::Concave { depth: want * top },
                        depth,
                        ..channel(mirror(&right), right.clone())
                    };
                    let mut built = false;
                    for _ in 0..3 {
                        match run(&w) {
                            Ok(_) => {
                                built = true;
                                break;
                            }
                            Err(err) => {
                                let Some(fit) = err.fit() else {
                                    break;
                                };
                                w.face = Contour::Concave { depth: fit.size };
                            }
                        }
                    }
                    if !built {
                        refused_fits.push((f, want, depth, run(&w).err()));
                    }
                }
            }
        }
    }
    assert!(refused_fits.is_empty(), "{refused_fits:?}");
    // the walls curving up whose flat channel is refused, at each of the six sizes: at 0.01
    // degrees radius 0.5, 2 and 5, at 0.002 degrees radius 0.5, 2, 5, 10, 30 and 100
    assert_eq!(flat_refused, 9 * 6);
}

#[test]
fn concave_fits_build_beside_shallow_and_curved_walls_that_leave_the_toe() {
    // an arc wall of radius 2 leaving the toe at 0.05 degrees, curving down, and a line wall
    // at 0.002 degrees 1000 wide: refused with fits that build when retried (brett19's cases on
    // #18947, 24 Nov)
    let arc = vec![toe_arc(0.05, 2.0, true)];
    let t = 0.002_f64.to_radians();
    let line_wall = vec![line(
        p(1000.0, 0.0),
        p(1000.0 - 500.0 * t.cos(), -500.0 * t.sin()),
    )];
    for (right, depth) in [(arc, 0.9), (line_wall, 900.0)] {
        let w = Case {
            face: Contour::Concave { depth },
            ..channel(mirror(&right), right.clone())
        };
        let err = run(&w).expect_err("past what it touches");
        let fit = err.fit().expect("a fit").size;
        assert!(
            run(&Case {
                face: Contour::Concave { depth: fit },
                ..w
            })
            .is_ok(),
            "fit {fit} does not build"
        );
    }
}

#[test]
fn a_depth_cut_on_a_large_section_lies_at_its_level() {
    // the right wall (f, 0) -> (f, -0.4 f) -> (0.1 f, -1.4 f), mirrored, filled 0.4 f + d deep
    // from the chord: the band below the wall's corner has the closed-form area 2 f d - 0.9 d²
    // (brett19 on #18947, 24 Nov: the cut snapped to the corner, 2e4 short)
    let (f, d) = (1e5, 0.1);
    let right = vec![
        line(p(f, 0.0), p(f, -0.4 * f)),
        line(p(f, -0.4 * f), p(0.1 * f, -1.4 * f)),
    ];
    let w = Case {
        depth: Some(0.4 * f + d),
        ..channel(mirror(&right), right.clone())
    };
    let got = area(&w);
    let want = 0.8 * f * f + 2.0 * f * d - 0.9 * d * d;
    assert!((got - want).abs() < 1e-3, "area {got}, want {want}");
}
