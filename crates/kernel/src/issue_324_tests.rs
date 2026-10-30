//! #324: a round on a block's top edge that runs over a through hole: built as if the top went
//! on, and cut by the hole.

use solidcraft_geom::Vec3;

use crate::{Body, BoolOp, build::box_solid, build::cylinder, ops::boolean};

/// A 40 x 20 x 10 block with a through hole of radius 3 at (20, `y`).
fn block_with_hole(y: f64) -> Body {
    let block = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 20.0, 10.0)).unwrap();
    let hole = cylinder(Vec3::new(20.0, y, -1.0), Vec3::Z, 3.0, 12.0).unwrap();
    boolean(&block, &hole, BoolOp::Cut).unwrap().unwrap()
}

/// The round on the top edge at y = 20 covers the top face from y = 20 - r, and the hole's
/// wall reaches y = 17.5: a radius over 2.5 runs into the hole. R4 is built as if the top went
/// on over the hole and cut from the block (the blend-overflow decision, 237f0ae74): the round
/// takes 16 (1 - π/4) of the section along 40, less its part over the hole, where the round's
/// height above its arc, 4 - √(16 - (y - 16)²), stands over the hole's width
/// 2 √(9 - (y - 14.5)²) for y in [16, 17.5] (integrated with y = 17.5 - s², smooth in s).
#[test]
fn a_round_into_a_hole_is_cut_by_it() {
    use std::f64::consts::{FRAC_PI_4, PI};
    let b = block_with_hole(14.5);
    let edge = Vec3::new(10.0, 20.0, 10.0);
    let r = crate::fillet(&b, &[edge], 4.0).unwrap();
    assert!(r.validity().is_empty(), "{:?}", r.validity());
    let (n, top) = (20000, 1.5f64.sqrt());
    let over: f64 = (0..n)
        .map(|i| {
            let s = (i as f64 + 0.5) * top / n as f64;
            let y = 17.5 - s * s;
            2.0 * (9.0 - (y - 14.5).powi(2)).sqrt() * (4.0 - (16.0 - (y - 16.0).powi(2)).sqrt()) * 2.0 * s
        })
        .sum::<f64>()
        * top
        / n as f64;
    let want = 8000.0 - PI * 9.0 * 10.0 - (40.0 * 16.0 * (1.0 - FRAC_PI_4) - over);
    let v = crate::measure(&r).unwrap().volume;
    assert!((v - want).abs() < 1e-7 * want, "{v} against {want}");
    // Clear of the hole (up to 2.5) the round keeps its whole section.
    let r = crate::fillet(&b, &[edge], 2.5).unwrap();
    let want = 8000.0 - PI * 9.0 * 10.0 - 40.0 * 2.5 * 2.5 * (1.0 - FRAC_PI_4);
    let v = r.volume_at(0.005).unwrap();
    assert!((v - want).abs() < 0.05, "{v} against {want}");
}
