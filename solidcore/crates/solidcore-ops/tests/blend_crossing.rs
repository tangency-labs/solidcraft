//! Rounds that overlap where a wall is thinner than both (SolidCraft #871): a plate with a
//! narrower block on top and a window through both, rounded along every edge of both end faces.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

mod common;

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::primitive::api_primitives::create_box;
use solidcore::Vec3;

use common::*;
use solidcore::kernel::Kernel;
use solidcore_ops::*;

const A: f64 = 12.744_373_655_385_26;
const B: f64 = 22.692_759_633_669_36;

/// The part from #871.
fn part() -> Body {
    built(|k| {
        let bx = |k: &mut Kernel, lo: [f64; 3], hi: [f64; 3]| {
            create_box(
                k,
                Vec3::new(lo[0], lo[1], lo[2]),
                hi[0] - lo[0],
                hi[1] - lo[1],
                hi[2] - lo[2],
            )
        };
        let base = bx(k, [9.5, 8.0, 0.0], [23.5, 29.5, 10.0])?;
        let top = bx(k, [9.5, A, 10.0], [23.5, B, 20.0])?;
        let joined = boolean_union(k, base, top)?;
        let window = bx(k, [9.5, 14.0, 7.0], [23.5, 19.2, 14.1])?;
        boolean_difference(k, joined, window)
    })
}

/// The 24 edges of both end faces.
fn end_edges(b: &Body) -> Vec<usize> {
    let yz = [
        (A, 15.0),
        (10.372_186_827_692_63, 10.0),
        (8.0, 5.0),
        (18.75, 0.0),
        (29.5, 5.0),
        (26.096_379_816_834_68, 10.0),
        (B, 15.0),
        (17.718_566_644_527_31, 20.0),
        (16.6, 7.0),
        (14.0, 10.55),
        (16.6, 14.1),
        (19.2, 10.55),
    ];
    [23.5, 9.5]
        .iter()
        .flat_map(|&x| yz.iter().map(move |&(y, z)| [x, y, z]))
        .map(|p| edge_at(b, p))
        .collect()
}

/// Radius 1 on both the window's edges and the block's sides: the wall between them is 1.26
/// thick, so the rounds would overlap and the end faces' boundaries cross. The kernel refuses
/// it before building (its bands overlap). Below half the wall the rounds fit (SolidCraft's
/// closed form: each end face's outline and window, 107.6 long together, give up
/// r^2 (1 - pi/4) per length at both ends, the mitred corners cancelling), and the rounds' faces
/// and their neighbours cross nowhere (`rounds_cross` passes, the check finds nothing).
#[test]
fn overlapping_rounds_are_refused_and_rounds_that_fit_close() {
    all_fixes();
    let p = part();
    let edges = end_edges(&p);
    let part_volume = 14.0 * (21.5 * 10.0 + (B - A) * 10.0 - 5.2 * 7.1);
    let v = p.volume(1e-8).unwrap();
    assert!((v - part_volume).abs() < 1e-9 * part_volume, "{v}");
    let c = p.copy().unwrap();
    match blend(&c, Blend::Round, &edges, &[1.0], &[], Corners::Filled) {
        Err(Error::TooLarge {
            why: Some(RoomReason::BandsOverlap),
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
    for r in [0.5, 0.62] {
        let c = p.copy().unwrap();
        blend(&c, Blend::Round, &edges, &[r], &[], Corners::Filled).unwrap();
        let length = 2.0 * (21.5 + 20.0) + 2.0 * (5.2 + 7.1);
        let removed = 2.0 * length * r * r * (1.0 - std::f64::consts::FRAC_PI_4);
        let v = c.volume(1e-8).unwrap();
        assert!(
            (v - (part_volume - removed)).abs() < 1e-7 * part_volume,
            "{r}: {v} vs {}",
            part_volume - removed
        );
        assert!(c.check(CheckLevel::All).unwrap().is_valid());
    }
}
