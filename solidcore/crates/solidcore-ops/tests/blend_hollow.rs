//! Rounds that would open into a hollow behind them (SolidCraft's `rounds_into_hollow` cases:
//! #1668's thin wall, #2852's U shell), run through [`blend`] with every fix on: the kernel's
//! `NoRoomReason::BreaksThrough` refuses them as [`Error::TooLarge`] with
//! [`RoomReason::BreaksThrough`].
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

mod common;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::primitive::api_primitives::create_box;
use solidcore::Vec3;

use common::*;
use solidcore_ops::*;

/// The box `[lo, hi]` less the box `[cut_lo, cut_hi]`.
fn box_less_box(lo: [f64; 3], hi: [f64; 3], cut_lo: [f64; 3], cut_hi: [f64; 3]) -> Body {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    built(|k| {
        let outer = create_box(k, v(lo), hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2])?;
        let inner = create_box(
            k,
            v(cut_lo),
            cut_hi[0] - cut_lo[0],
            cut_hi[1] - cut_lo[1],
            cut_hi[2] - cut_lo[2],
        )?;
        boolean_difference(k, outer, inner)
    })
}

/// #1668: a 104 x 64 x 42 box hollowed 2 mm from the inside, open at the bottom.
fn thin_walled() -> Body {
    box_less_box(
        [-52.0, -32.0, 0.0],
        [52.0, 32.0, 42.0],
        [-50.0, -30.0, -1.0],
        [50.0, 30.0, 40.0],
    )
}

/// #2852: a 3 mm upside-down U, 60 x 20 x 50, its inside top edges rounded 15.
fn u_shell() -> Body {
    let b = box_less_box(
        [0.0, 0.0, 0.0],
        [60.0, 20.0, 50.0],
        [3.0, -1.0, -1.0],
        [57.0, 21.0, 47.0],
    );
    let inside = [
        edge_at(&b, [3.0, 10.0, 47.0]),
        edge_at(&b, [57.0, 10.0, 47.0]),
    ];
    blend(&b, Blend::Round, &inside, &[15.0], &[], Corners::Filled).unwrap();
    b
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

const QUARTER: f64 = 1.0 - std::f64::consts::FRAC_PI_4;

/// The round of `edges` at `sizes` on a copy of `b`: the copy's volume, or the refusal.
fn round(b: &Body, edges: &[usize], sizes: &[f64]) -> Result<f64> {
    let c = b.copy().unwrap();
    blend(&c, Blend::Round, edges, sizes, &[], Corners::Filled).map(|_| volume(&c))
}

/// A refusal for breaking through, its fit within 0.011 below `limit` (the kernel's fit
/// margin, 0.01, under its bound), at the middle of the edge `mid`.
fn breaks_through(r: Result<f64>, limit: f64, mid: [f64; 3]) {
    match r {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            at: Some(at),
            why: Some(RoomReason::BreaksThrough),
        }) => {
            assert!(size < limit && limit - size < 0.011, "{size} vs {limit}");
            assert!(
                at.iter().zip(mid).all(|(a, m)| (a - m).abs() < 1e-9),
                "{at:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

fn close(v: f64, want: f64) {
    assert!((v - want).abs() <= 1e-7 * want, "{v} vs {want}");
}

/// #1668: the outer top edge's round, centre (32 - r, 42 - r) in (y, z), uncovers the hollow's
/// corner (30, 40) at sqrt2 (r - 2) = r, r = 4 + 2 sqrt2: R7 is refused for breaking through,
/// naming a radius just under it; R5 and R6.8 build, taking (1 - pi/4) r^2 along 104 off
/// 104 64 42 - 100 60 40 = 39552. With the inside edge rounded R5 too, which adds
/// (1 - pi/4) 25 along 100.
#[test]
fn a_round_through_a_thin_wall_is_refused_as_breaking_through() {
    all_fixes();
    let wall = thin_walled();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    let limit = 4.0 + 2.0 * std::f64::consts::SQRT_2;
    breaks_through(round(&wall, &[top], &[7.0]), limit, [0.0, 32.0, 42.0]);
    for r in [5.0, 6.8] {
        close(
            round(&wall, &[top], &[r]).unwrap(),
            39552.0 - QUARTER * r * r * 104.0,
        );
    }
    close(
        round(&wall, &[top, inside], &[5.0]).unwrap(),
        39552.0 - QUARTER * 25.0 * 4.0,
    );
}

/// #2852: a 3 mm U whose inside top edges are rounded R15 (centres (18, 32), (42, 32) in
/// (x, z)): its outside rounds' arcs reach the inside rounds' at R = (18 sqrt2 - 15) /
/// (sqrt2 - 1) = 25.2426. R30 is refused for breaking through, naming a radius just under
/// it; R18 (concentric) and R25 build, each taking (1 - pi/4) R^2 along 20 at both edges
/// off 60 20 50 - 54 20 47 + 2 (1 - pi/4) 225 20.
#[test]
fn a_round_past_an_inside_round_is_refused_as_breaking_through() {
    all_fixes();
    let u = u_shell();
    let base = 60.0 * 20.0 * 50.0 - 54.0 * 20.0 * 47.0 + 2.0 * QUARTER * 225.0 * 20.0;
    close(volume(&u), base);
    let outer = [
        edge_at(&u, [0.0, 10.0, 50.0]),
        edge_at(&u, [60.0, 10.0, 50.0]),
    ];
    let s2 = std::f64::consts::SQRT_2;
    let limit = (18.0 * s2 - 15.0) / (s2 - 1.0);
    match round(&u, &outer, &[30.0]) {
        Err(Error::TooLarge {
            size,
            why: Some(RoomReason::BreaksThrough),
            ..
        }) => assert!(size < limit && limit - size < 0.011, "{size} vs {limit}"),
        other => panic!("{other:?}"),
    }
    for r in [18.0, 25.0] {
        close(
            round(&u, &outer, &[r]).unwrap(),
            base - 2.0 * QUARTER * r * r * 20.0,
        );
    }
}

/// SolidCraft builds these (its `rounds_into_hollow` counts the inside edge's concave round,
/// made in the same fillet, as material the outer round runs over: the rig's
/// 1668-fillet-both-sides-of-thin-wall-r7, 39509.938); the kernel's `BreaksThrough` judges the
/// outer round against the body before the inside round is added and refuses them. Measured
/// in notes, not yet the kernel's: ignored until `BreaksThrough` counts concave rounds of the
/// same blend.
#[test]
#[ignore = "BreaksThrough judges a round before the concave rounds of the same blend add material"]
fn an_inside_round_holds_the_wall_an_outer_round_runs_over() {
    all_fixes();
    let wall = thin_walled();
    let top = edge_at(&wall, [0.0, 32.0, 42.0]);
    let inside = edge_at(&wall, [0.0, 30.0, 40.0]);
    close(
        round(&wall, &[top, inside], &[7.0]).unwrap(),
        39_509.938_040_025_9,
    );
    assert!(round(&wall, &[top, inside], &[13.8, 7.0]).is_ok());
}
