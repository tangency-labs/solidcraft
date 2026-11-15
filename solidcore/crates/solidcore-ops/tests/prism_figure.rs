//! The sheet figures through the record: a figure placed and turned, a regular outline and a
//! label's box inside an outline, each reaching the kernel's `prism::figure` and refusing hostile
//! values. The oracles are closed forms.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_methods,
    clippy::float_cmp
)]

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore_ops::prism::{
    label_in_outline, place_figure, regular_outline, LabelFit, PlacedFigure,
};
use solidcore_ops::Error;
use solidcore_record::{Seg2, Vec2};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn sheet_figures_reach_the_kernel_through_the_record() {
    // A figure placed at 3.5 about (100, 50): the arc's centre moves, its radius scales and its
    // angles stay. The values are exact in binary.
    let figure = [
        Seg2::Line {
            a: v(0.0, 0.0),
            b: v(1.0, 0.0),
        },
        Seg2::Arc {
            center: v(0.5, 0.5),
            radius: 0.5,
            start: 0.0,
            sweep: PI,
        },
    ];
    let placed = place_figure(&figure, &[v(0.0, 1.0)], 3.5, 0.0, v(100.0, 50.0)).unwrap();
    let line = Seg2::Line {
        a: v(100.0, 50.0),
        b: v(103.5, 50.0),
    };
    let arc = Seg2::Arc {
        center: v(101.75, 51.75),
        radius: 1.75,
        start: 0.0,
        sweep: PI,
    };
    let want = PlacedFigure {
        segments: vec![line, arc],
        points: vec![v(100.0, 53.5)],
        direction: v(1.0, 0.0),
    };
    assert_eq!(placed, want, "no turn: exactly the scaled and moved figure");
    // A quarter turn through the record: the line points up, and the arc starts a quarter on.
    let turned = place_figure(&figure, &[], 1.0, FRAC_PI_2, v(0.0, 0.0)).unwrap();
    let Seg2::Line { b, .. } = turned.segments[0] else {
        panic!("a line stays a line")
    };
    assert!(b.x.abs() < 1e-15 && (b.y - 1.0).abs() < 1e-15, "{b:?}");
    let Seg2::Arc { start, sweep, .. } = turned.segments[1] else {
        panic!("an arc stays an arc")
    };
    assert_eq!((start, sweep), (FRAC_PI_2, PI));
    // A revision tag's triangle: side 6 about (100, 50), point up, counter-clockwise.
    let r3 = 3.0_f64.sqrt();
    let tri = regular_outline(v(100.0, 50.0), 3, 6.0, FRAC_PI_2).unwrap();
    let want = [
        v(100.0, 50.0 + 2.0 * r3),
        v(97.0, 50.0 - r3),
        v(103.0, 50.0 - r3),
    ];
    assert_eq!(tri.len(), 3);
    for (got, want) in tri.iter().zip(want) {
        assert!(
            (got.x - want.x).abs() < 1e-12 && (got.y - want.y).abs() < 1e-12,
            "{got:?}"
        );
    }
    // A box twice as wide as tall in [0, 20] x [0, 10], a margin of 1: the top and bottom
    // margins leave 8, so the box is 16 wide, from x = 2.
    let wide = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)];
    let fit = label_in_outline(&wide, 10.0, 2.0, 1.0, 100.0).unwrap();
    let want = LabelFit {
        height: 8.0,
        min: v(2.0, 1.0),
    };
    assert_eq!(fit, Some(want));
    assert_eq!(
        label_in_outline(&wide, 10.0, 2.0, 6.0, 100.0).unwrap(),
        None
    );
}

#[test]
fn sheet_figures_refuse_hostile_values_through_the_record() {
    let line = [Seg2::Line {
        a: v(0.0, 0.0),
        b: v(1.0, 0.0),
    }];
    assert!(matches!(
        place_figure(&line, &[], -1.0, 0.0, v(0.0, 0.0)),
        Err(Error::Status(_))
    ));
    assert!(matches!(
        place_figure(&line, &[], 1.0, f64::NAN, v(0.0, 0.0)),
        Err(Error::Status(_))
    ));
    assert!(matches!(
        regular_outline(v(0.0, 0.0), 2, 1.0, 0.0),
        Err(Error::Status(_))
    ));
    let clockwise = [v(0.0, 0.0), v(0.0, 10.0), v(10.0, 10.0), v(10.0, 0.0)];
    let got = label_in_outline(&clockwise, 5.0, 2.0, 1.0, 100.0);
    assert!(matches!(got, Err(Error::Status(_))), "{got:?}");
    let got = label_in_outline(&clockwise[..2], 5.0, 2.0, 1.0, 100.0);
    assert!(matches!(got, Err(Error::Status(_))), "{got:?}");
}
