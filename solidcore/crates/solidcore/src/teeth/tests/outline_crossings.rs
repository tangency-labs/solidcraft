//! The outline's own backstop: `Outline::new` and `Body::new` refuse an outline that crosses itself
//! or does not run counter-clockwise around an area, and accept every outline that only touches
//! itself, quickly even when it is large. Each refusal and each acceptance is checked against the
//! slow way: every chord against every other.

use std::f64::consts::{PI, TAU};

use super::common::{why, Dice};
use crate::error::teeth::Why;
use crate::teeth::mesh::{Body, Material};
use crate::teeth::{Outline, Piece, Point, Role};

fn line(from: Point, to: Point) -> Piece {
    Piece::Line {
        from,
        to,
        role: Role::Flank,
    }
}

/// A closed polygon of lines as an outline of one tooth.
fn polygon(points: &[Point]) -> crate::Result<Outline> {
    let tooth = (0..points.len())
        .map(|i| line(points[i], points[(i + 1) % points.len()]))
        .collect();
    Outline::new(1, tooth, 1e-3)
}

/// Whether two chords cross, each passing from one side of the other to the other side.
fn cross(p: Point, q: Point, u: Point, v: Point) -> bool {
    let s = |f: Point, t: Point, o: Point| {
        (t[0] - f[0]) * (o[1] - f[1]) - (t[1] - f[1]) * (o[0] - f[0])
    };
    s(u, v, p) * s(u, v, q) < 0.0 && s(p, q, u) * s(p, q, v) < 0.0
}

/// How many pairs of chords of a closed chain cross, the slow way.
fn crossings(points: &[Point]) -> usize {
    let n = points.len();
    let mut count = 0;
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if cross(
                points[i],
                points[(i + 1) % n],
                points[j],
                points[(j + 1) % n],
            ) {
                count += 1;
            }
        }
    }
    count
}

fn refused_as_crossing(outline: &crate::Result<Outline>) -> bool {
    why(outline) == Some(Why::CrossesItself)
}

#[test]
fn shapes_that_cross_themselves_or_run_the_wrong_way_are_refused() {
    // A pentagram: each point joined to the one two on.
    let star: Vec<Point> = (0..5)
        .map(|k| {
            [
                10.0 * (TAU * f64::from(2 * k) / 5.0).cos(),
                10.0 * (TAU * f64::from(2 * k) / 5.0).sin(),
            ]
        })
        .collect();
    assert!(refused_as_crossing(&polygon(&star)));
    assert!(Body::new(star.clone(), Material::Inside).is_err());
    // A bow tie has no area to run around; a square run clockwise runs the wrong way.
    let bow = [[1.0, 1.0], [-1.0, -1.0], [-1.0, 1.0], [1.0, -1.0]];
    assert!(polygon(&bow).is_err() && Body::new(bow.to_vec(), Material::Inside).is_err());
    let clockwise = [[1.0, 0.0], [0.0, -1.0], [-1.0, 0.0], [0.0, 1.0]];
    assert_eq!(why(&polygon(&clockwise)), Some(Why::NotCounterClockwise));
    assert!(Body::new(clockwise.to_vec(), Material::Outside).is_err());
    // A figure of eight with one loop larger, so that its area is positive: it still crosses.
    let eight = [[2.0, 2.0], [-1.0, -1.0], [-1.0, 1.0], [2.0, -2.0]];
    assert!(refused_as_crossing(&polygon(&eight)));
    assert!(Body::new(eight.to_vec(), Material::Inside).is_err());
    // A spiral of lines whose last stretches run across its first.
    let spiral = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 4.0],
        [2.0, 4.0],
        [2.0, -2.0],
        [1.0, -2.0],
        [1.0, 3.0],
        [0.0, 3.0],
    ];
    assert!(crossings(&spiral) > 0);
    assert!(refused_as_crossing(&polygon(&spiral)));
    // A chain that runs through one of its own corners from one side to the other.
    let corner_on_corner = [
        [0.0, 0.0],
        [2.0, 0.0],
        [2.0, 2.0],
        [1.0, 1.0],
        [0.0, 2.0],
        [0.0, 1.0],
        [1.0, 1.0],
        [1.0, 3.0],
        [-1.0, 3.0],
        [-1.0, 0.0],
    ];
    assert!(Body::new(corner_on_corner.to_vec(), Material::Inside).is_err());
    // A corner lying on another chord with the chain passing through it there.
    let corner_on_chord = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 2.0],
        [2.0, 0.0],
        [2.0, -1.0],
        [5.0, -1.0],
        [5.0, 3.0],
        [0.0, 3.0],
    ];
    assert!(Body::new(corner_on_chord.to_vec(), Material::Inside).is_err());
}

#[test]
fn shapes_that_only_touch_themselves_are_accepted() -> crate::Result<()> {
    // Two squares that touch at one corner, run as one chain through that corner twice.
    let corners = [
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [2.0, 1.0],
        [2.0, 2.0],
        [1.0, 2.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];
    polygon(&corners)?;
    Body::new(corners.to_vec(), Material::Inside)?;
    // A corner of the chain resting on a straight stretch of it, from inside.
    let resting = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 3.0],
        [3.0, 3.0],
        [2.0, 0.0001],
        [1.0, 3.0],
        [0.0, 3.0],
    ];
    polygon(&resting)?;
    let touching = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 3.0],
        [3.0, 3.0],
        [2.0, 0.0],
        [1.0, 3.0],
        [0.0, 3.0],
    ];
    polygon(&touching)?;
    Body::new(touching.to_vec(), Material::Inside)?;
    // A slit: the chain runs out along a line and back along it.
    let slit = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 2.0],
        [2.0, 2.0],
        [2.0, 1.0],
        [2.0, 2.0],
        [0.0, 2.0],
    ];
    Body::new(slit.to_vec(), Material::Inside)?;
    // Arcs that meet a line tangentially and teeth whose arcs touch the next tooth's arcs.
    let rounded = Outline::new(
        4,
        vec![
            line([1.0, -0.6], [1.0, 0.6]),
            Piece::Arc {
                centre: [0.6, 0.6],
                radius: 0.4,
                start: 0.0,
                sweep: PI / 2.0,
                role: Role::Fillet,
            },
        ],
        1e-6,
    )?;
    Body::new(rounded.points(), Material::Inside)?;
    // Teeth that touch: each tooth most of a circle, each circle touching the next one's at the
    // point where the outline passes from one tooth to the next.
    for teeth in [2_u32, 3, 8, 40, 2000] {
        let pitch = TAU / f64::from(teeth);
        let radius = 10.0 * (pitch / 2.0).sin();
        let start = -(PI / 2.0 + pitch / 2.0);
        let tooth = vec![Piece::Arc {
            centre: [10.0, 0.0],
            radius,
            start,
            sweep: PI + pitch,
            role: Role::Tip,
        }];
        let outline = Outline::new(teeth, tooth, 1e-4);
        assert!(outline.is_ok(), "{teeth} touching teeth: {outline:?}");
    }
    // A crescent: a circle with a round opening that touches it from inside, run as one chain
    // through the place where they touch. The chords of the two circles there run into each other
    // by less than the tolerance.
    for tolerance in [0.1, 1e-2, 1e-3, 1e-4, 1e-5, 1e-7] {
        for inner in [0.2, 0.5, 1.0, 1.5, 1.9, 1.99] {
            let crescent = vec![
                Piece::Arc {
                    centre: [0.0, 0.0],
                    radius: 2.0,
                    start: 0.0,
                    sweep: TAU,
                    role: Role::Tip,
                },
                Piece::Arc {
                    centre: [2.0 - inner, 0.0],
                    radius: inner,
                    start: 0.0,
                    sweep: -TAU,
                    role: Role::Root,
                },
            ];
            let outline = Outline::new(1, crescent, tolerance);
            assert!(
                outline.is_ok(),
                "crescent {inner} at {tolerance}: {outline:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn random_polygons_are_judged_as_the_slow_way_judges_them() {
    let mut dice = Dice(0xC055);
    let (mut crossing, mut simple) = (0, 0);
    for round in 0..3000 {
        let n = 3 + dice.roll() % 9;
        let points: Vec<Point> = if round % 2 == 0 {
            // Any points at all: these mostly cross.
            (0..n)
                .map(|_| {
                    [
                        (dice.roll() % 1000) as f64 / 100.0,
                        (dice.roll() % 1000) as f64 / 100.0,
                    ]
                })
                .collect()
        } else {
            // A star about the origin, every ray meeting it once: these never cross.
            (0..n)
                .map(|k| {
                    let angle =
                        TAU * (k as f64 + 0.4 * (dice.roll() % 1000) as f64 / 1000.0) / n as f64;
                    let radius = 1.0 + (dice.roll() % 1000) as f64 / 100.0;
                    [radius * angle.cos(), radius * angle.sin()]
                })
                .collect()
        };
        let area: f64 = (0..n)
            .map(|i| points[i][0] * points[(i + 1) % n][1] - points[(i + 1) % n][0] * points[i][1])
            .sum();
        let expected = area > 0.0 && crossings(&points) == 0;
        // Points on a grid of hundredths can lie exactly on each other's chords; the slow way
        // counts only proper crossings there, so those are left out.
        let on_a_chord = (0..n).any(|i| {
            (0..n).any(|j| {
                let (u, v) = (points[j], points[(j + 1) % n]);
                let p = points[i];
                i != j
                    && i != (j + 1) % n
                    && ((v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])).abs() < 1e-9
                    && (p[0] - u[0]) * (p[0] - v[0]) + (p[1] - u[1]) * (p[1] - v[1]) <= 0.0
            })
        });
        if on_a_chord {
            continue;
        }
        let body = Body::new(points.clone(), Material::Inside).is_ok();
        let outline = polygon(&points).is_ok();
        assert_eq!(body, expected, "round {round}: {points:?}");
        assert_eq!(outline, expected, "round {round}: {points:?}");
        if expected {
            simple += 1;
        } else {
            crossing += 1;
        }
    }
    assert!(crossing > 500 && simple > 1000, "{crossing} {simple}");
}

#[test]
fn large_outlines_are_checked_in_good_time() -> crate::Result<()> {
    // One tooth of eight million points: a circle as 160 curves of 50,000 points each.
    let pieces = 160;
    let each = 50_000;
    let tooth: Vec<Piece> = (0..pieces)
        .map(|k| {
            let points = (0..each)
                .map(|i| {
                    let a = TAU * (f64::from(k) + f64::from(i) / f64::from(each - 1))
                        / f64::from(pieces);
                    [100.0 * a.cos(), 100.0 * a.sin()]
                })
                .collect();
            Piece::Curve {
                points,
                role: Role::Tip,
            }
        })
        .collect();
    let outline = Outline::new(1, tooth, 1e-3)?;
    assert!(outline.points().len() > 7_900_000);
    // Two thousand teeth of four thousand points each, the teeth close together.
    let pitch = TAU / 2000.0;
    let points: Vec<Point> = (0..4000)
        .map(|i| {
            let t = f64::from(i) / 3999.0;
            let a = pitch * t;
            let r = 1000.0 + 2.0 * (PI * t).sin();
            [r * a.cos(), r * a.sin()]
        })
        .collect();
    let outline = Outline::new(
        2000,
        vec![Piece::Curve {
            points,
            role: Role::Tip,
        }],
        1e-3,
    )?;
    assert_eq!(outline.teeth(), 2000);
    // As a body (checked whole, without the teeth's sameness).
    Body::new(outline.points(), Material::Inside)?;
    Ok(())
}
