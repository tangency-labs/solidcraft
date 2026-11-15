//! Tests of the sheet figures: the closed forms of a placed figure, a regular outline's corners and
//! the label box that fits inside an outline, and their hostile inputs refused.

#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use super::super::profile::{
    check_regions, ProfileRefusal, Seg2, MAX_BEZIER_CTRL, MAX_PROFILE_COORD, MAX_SEGMENTS,
};
use super::{
    box_fit_in_convex_polygon, place_figure_2d, regular_polygon_2d, BoxFit, FigurePlacement,
};
use crate::math::Point2;
use crate::prism::tests::rectangle;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

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
