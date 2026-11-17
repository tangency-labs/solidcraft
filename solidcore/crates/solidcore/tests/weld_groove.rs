//! Groove welds (`weld::groove`): the section of every standard groove and of grooves that
//! match no standard against closed-form areas, the contour of each face (a weld from one side
//! has a flat root, a weld from both sides a second face of its own), the penetration depth and
//! its notices, and the refusals.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore::math::Point2;
use solidcore::prism::profile::Seg2;
use solidcore::weld::groove::{
    groove_section, Contour, GrooveNotice, GrooveSection, GrooveWeld, WeldSide,
};
use solidcore::SolidError;

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

fn weld(left: Vec<Seg2>, right: Vec<Seg2>) -> GrooveWeld {
    GrooveWeld {
        left,
        right,
        ..GrooveWeld::default()
    }
}

fn section(w: &GrooveWeld) -> GrooveSection {
    groove_section(w).unwrap()
}

fn area(w: &GrooveWeld) -> f64 {
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
    let v = weld(l, r);
    close(area(&v), G * t + t * t * tan(30.0));
    let s = section(&v);
    assert!(s.through);
    assert!((s.groove_depth - t).abs() < 1e-12);
    assert!(s.notices.is_empty());

    let (l, r) = bevel_walls(t, 45.0, 0.0);
    close(area(&weld(l, r)), G * t + t * t / 2.0);

    let c = 2.0;
    let (l, r) = v_walls(t, 60.0, c);
    close(area(&weld(l, r)), G * t + (t - c) * (t - c) * tan(30.0));

    let (l, r) = bevel_walls(t, 45.0, c);
    close(area(&weld(l, r)), G * t + (t - c) * (t - c) / 2.0);

    let right = u_right();
    close(
        area(&weld(mirror(&right), right.clone())),
        G * U_T + 2.0 * u_half_area(),
    );
    let square = vec![line(p(-G / 2.0, 0.0), p(-G / 2.0, -U_T))];
    close(area(&weld(square, right)), G * U_T + u_half_area());

    let (t2, c2) = (12.0, 2.0);
    let d = (t2 - c2) / 2.0;
    let (l, r) = x_walls(t2, c2);
    close(area(&weld(l, r)), G * t2 + 2.0 * d * d * tan(30.0));

    let k_right = vec![
        line(p(G / 2.0 + d, 0.0), p(G / 2.0, -d)),
        line(p(G / 2.0, -d), p(G / 2.0, -d - c2)),
        line(p(G / 2.0, -d - c2), p(G / 2.0 + d, -t2)),
    ];
    let k_left = vec![line(p(-G / 2.0, 0.0), p(-G / 2.0, -t2))];
    close(area(&weld(k_left, k_right)), G * t2 + d * d);
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
    close(area(&weld(left, right)), shoelace);

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
    close(area(&weld(left, right)), 36.0 + 1.0 - PI / 4.0);
}

#[test]
fn convex_faces_add_their_segments_and_a_one_sided_root_stays_flat() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let groove = G * t + t * t * tan(30.0);
    let half = G / 2.0 + t * tan(30.0);
    let w = GrooveWeld {
        face: Contour::Convex { height: 1.5 },
        ..weld(l.clone(), r.clone())
    };
    close(area(&w), groove + segment(half, 1.5));
    // welded from one side, the root closes the 1 mm gap flat: nothing below the plate
    let s = section(&w);
    let lowest = s
        .region
        .outer
        .segs
        .iter()
        .flat_map(|seg| (0..=16).map(move |i| seg.point_at(f64::from(i) / 16.0).y))
        .fold(f64::MAX, f64::min);
    assert!((lowest + t).abs() < 1e-12, "root at {lowest}");
    assert!(s.notices.is_empty());

    // welded from both sides: each face follows its own contour, flat by default
    let (t2, c2) = (12.0, 2.0);
    let d = (t2 - c2) / 2.0;
    let (l, r) = x_walls(t2, c2);
    let half = G / 2.0 + d * tan(30.0);
    let x_groove = G * t2 + 2.0 * d * d * tan(30.0);
    let w = GrooveWeld {
        face: Contour::Convex { height: 1.0 },
        second_face: Some(Contour::Convex { height: 1.2 }),
        ..weld(l.clone(), r.clone())
    };
    close(area(&w), x_groove + segment(half, 1.0) + segment(half, 1.2));
    let w = GrooveWeld {
        face: Contour::Convex { height: 1.0 },
        second_face: Some(Contour::Flat),
        ..weld(l.clone(), r.clone())
    };
    close(area(&w), x_groove + segment(half, 1.0));
    let w = GrooveWeld {
        second_face: Some(Contour::Convex { height: 1.2 }),
        ..weld(l, r)
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
    // the default overlap is the crest height
    let w = GrooveWeld {
        face: Contour::ConvexBlended {
            height: h,
            overlap: None,
        },
        ..weld(l.clone(), r.clone())
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
    let w = GrooveWeld {
        face: Contour::ConvexBlended {
            height: h,
            overlap: Some(0.5),
        },
        ..weld(l.clone(), r.clone())
    };
    close(area(&w), groove + blended_cap(a, h, 0.5));

    // no overlap is the plain convex face
    let w = GrooveWeld {
        face: Contour::ConvexBlended {
            height: h,
            overlap: Some(0.0),
        },
        ..weld(l, r)
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
    let w = GrooveWeld {
        face: Contour::Concave { depth: 1.0 },
        ..weld(l, r)
    };
    close(area(&w), groove - segment(a, 1.0));
    assert_eq!(
        section(&w).notices,
        vec![GrooveNotice::BelowToes {
            side: WeldSide::Face,
            depth: 1.0,
            ratio: 1.0 / 6.0,
        }]
    );
    // a weld from both sides with its second face concave says so for that side
    let (t2, c2) = (12.0, 2.0);
    let d = (t2 - c2) / 2.0;
    let (xl, xr) = x_walls(t2, c2);
    let w = GrooveWeld {
        second_face: Some(Contour::Concave { depth: 0.5 }),
        ..weld(xl, xr)
    };
    close(
        area(&w),
        G * t2 + 2.0 * d * d * tan(30.0) - segment(G / 2.0 + d * tan(30.0), 0.5),
    );
    assert_eq!(
        section(&w).notices,
        vec![GrooveNotice::BelowToes {
            side: WeldSide::Second,
            depth: 0.5,
            ratio: 0.5 / 12.0,
        }]
    );
}

#[test]
fn a_concave_face_across_the_walls_is_refused_with_the_deepest_that_builds() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let a = G / 2.0 + t * tan(30.0);
    let w = GrooveWeld {
        face: Contour::Concave { depth: 3.0 },
        ..weld(l, r)
    };
    let err = groove_section(&w).unwrap_err();
    // the arc leaves the 60 degree V where its end tangent is steeper than the wall: at
    // 2 atan(d / a) = 60 degrees
    let fit = err
        .fit()
        .expect("the refusal says the depth that builds")
        .size;
    assert!((fit - a * tan(30.0)).abs() < 1e-3, "fit {fit}");
    let builds = GrooveWeld {
        face: Contour::Concave { depth: fit },
        ..w
    };
    assert!(groove_section(&builds).is_ok());
}

#[test]
fn a_penetration_depth_fills_the_groove_only_that_deep() {
    let t = 6.0;
    let (l, r) = v_walls(t, 60.0, 0.0);
    let s = 4.0;
    // a second face does not apply: the weld ends inside the groove
    let w = GrooveWeld {
        depth: Some(s),
        second_face: Some(Contour::Convex { height: 0.8 }),
        ..weld(l.clone(), r.clone())
    };
    close(area(&w), G * s + tan(30.0) * (t * t - (t - s) * (t - s)));
    assert!(section(&w).notices.is_empty());

    // as deep as the plate: full penetration, nothing to say
    let w = GrooveWeld {
        depth: Some(t),
        ..weld(l.clone(), r.clone())
    };
    close(area(&w), G * t + t * t * tan(30.0));
    assert!(section(&w).notices.is_empty());

    // past the plate: full penetration, and the user is told
    let w = GrooveWeld {
        depth: Some(8.0),
        ..weld(l, r)
    };
    close(area(&w), G * t + t * t * tan(30.0));
    assert_eq!(
        section(&w).notices,
        vec![GrooveNotice::DeeperThanPlate {
            asked: 8.0,
            plate: 6.0
        }]
    );

    // a U groove cut on its round
    let right = u_right();
    let w = GrooveWeld {
        depth: Some(8.0),
        ..weld(mirror(&right), right)
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
    let w = GrooveWeld {
        depth: Some(8.0),
        second_face: Some(Contour::Convex { height: 1.0 }),
        ..weld(left, right)
    };
    let s = section(&w);
    assert!(!s.through);
    close(area(&w), 36.0 * tan(30.0));
    assert_eq!(
        s.notices,
        vec![GrooveNotice::DeeperThanGroove {
            asked: 8.0,
            groove: 6.0
        }]
    );
}

#[test]
fn bad_walls_contours_and_depths_are_refused_as_invalid_input() {
    let (l, r) = v_walls(6.0, 60.0, 0.0);
    let ok = weld(l.clone(), r.clone());
    let cubic = Seg2::Cubic {
        p0: p(-4.0, 0.0),
        p1: p(-3.0, -2.0),
        p2: p(-1.0, -4.0),
        p3: p(-0.5, -6.0),
    };
    let bad = [
        weld(Vec::new(), r.clone()),
        weld(vec![cubic], r.clone()),
        // a wall whose two pieces do not join
        weld(
            vec![
                line(p(-4.0, 0.0), p(-2.0, -3.0)),
                line(p(-1.9, -3.0), p(-0.5, -6.0)),
            ],
            r.clone(),
        ),
        weld(vec![line(p(f64::NAN, 0.0), p(-0.5, -6.0))], r.clone()),
        // left and right swapped
        weld(r.clone(), l.clone()),
        // walls that cross each other
        weld(
            vec![line(p(-4.0, 0.0), p(2.0, -6.0))],
            vec![line(p(4.0, 0.0), p(-2.0, -6.0))],
        ),
        GrooveWeld {
            face: Contour::Convex { height: 0.0 },
            ..ok.clone()
        },
        GrooveWeld {
            face: Contour::Concave { depth: -1.0 },
            ..ok.clone()
        },
        GrooveWeld {
            second_face: Some(Contour::ConvexBlended {
                height: 1.0,
                overlap: Some(f64::NAN),
            }),
            ..ok.clone()
        },
        GrooveWeld {
            depth: Some(0.0),
            ..ok.clone()
        },
    ];
    for (i, w) in bad.iter().enumerate() {
        let err = groove_section(w).unwrap_err();
        assert!(
            matches!(err, SolidError::InvalidInput { .. }),
            "case {i}: {err:?}"
        );
    }
    assert!(groove_section(&ok).is_ok());
}
