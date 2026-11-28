//! Overlaps that no corner shows: long straight edges that cross, slivers thinner than the reach,
//! and a part lying wholly in the other's material. And the opposite: parts that only touch do not
//! overlap.

use crate::teeth::mesh::{gap, Body, Material};

/// A bar with its sides along the axes: from (x0, y0) to (x1, y1), counter-clockwise.
fn bar(x0: f64, y0: f64, x1: f64, y1: f64, material: Material) -> crate::Result<Body> {
    Body::new(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], material)
}

fn found(a: &Body, b: &Body, reach: f64) -> crate::Result<f64> {
    Ok(gap(a, b, reach)?.map_or(f64::NAN, |g| g.distance))
}

#[test]
fn two_long_bars_that_cross_overlap_though_no_corner_is_inside_the_other() -> crate::Result<()> {
    // A plus sign: each bar 100 long and 2 wide. Every corner of each is 49 mm outside the other.
    let across = bar(-50.0, -1.0, 50.0, 1.0, Material::Inside)?;
    let upright = bar(-1.0, -50.0, 1.0, 50.0, Material::Inside)?;
    for reach in [0.3, 1.0, 5.0, 60.0] {
        let depth = found(&across, &upright, reach)?;
        // The middle of the crossing is 1 mm inside each bar.
        assert!(depth < 0.0, "reach {reach}: {depth}");
        assert!(depth >= -1.0 - 1e-9, "reach {reach}: {depth}");
        if reach >= 1.0 {
            assert!(
                (depth + 1.0).abs() < 0.26,
                "reach {reach}: the depth found is {depth}, the bars are 1 deep in each other"
            );
        }
        assert!(
            found(&upright, &across, reach)? < 0.0,
            "reach {reach}: the other way round"
        );
    }
    // The same turned by any angle.
    for step in 0..40 {
        let turned = upright.placed(f64::from(step) * 0.077, [3.0, 0.2]);
        assert!(found(&across, &turned, 0.5)? < 0.0, "step {step}");
    }
    Ok(())
}

#[test]
fn a_sliver_thinner_than_the_reach_is_still_an_overlap() -> crate::Result<()> {
    // A blade 0.004 thick through a bar 2 thick, looked at with a reach of half a millimetre and
    // more.
    let thick = bar(-50.0, -1.0, 50.0, 1.0, Material::Inside)?;
    let blade = bar(-0.002, -50.0, 0.002, 50.0, Material::Inside)?;
    // The blade's own sides lie up to 1 mm deep in the bar: the depth found is that, as far as the
    // reach goes.
    for (reach, deepest) in [(0.5, 0.5), (2.0, 1.0)] {
        let depth = found(&thick, &blade, reach)?;
        assert!(
            depth < 0.0 && depth >= -deepest - 1e-9,
            "reach {reach}: {depth}"
        );
        assert!(
            found(&blade, &thick, reach)? < 0.0,
            "reach {reach}: the other way round"
        );
    }
    // A corner clipped by a long edge: a square turned an eighth, its tip 0.003 into a bar.
    let tip = 2f64.sqrt() - 0.003;
    let square = bar(-1.0, -1.0, 1.0, 1.0, Material::Inside)?
        .placed(std::f64::consts::FRAC_PI_4, [0.0, 0.0]);
    let wall = bar(-80.0, tip, 80.0, tip + 5.0, Material::Inside)?;
    let depth = found(&square, &wall, 1.0)?;
    assert!((depth + 0.003).abs() < 1e-9, "{depth}");
    Ok(())
}

#[test]
fn parts_that_only_touch_do_not_overlap() -> crate::Result<()> {
    let left = bar(-10.0, -1.0, 0.0, 1.0, Material::Inside)?;
    // Side against side, corner against side, corner against corner, and a T: all touch, none
    // overlaps.
    for (name, other) in [
        ("side on side", bar(0.0, -1.0, 10.0, 1.0, Material::Inside)?),
        (
            "side on a longer side",
            bar(0.0, -40.0, 3.0, 40.0, Material::Inside)?,
        ),
        (
            "corner on corner",
            bar(0.0, 1.0, 10.0, 3.0, Material::Inside)?,
        ),
        (
            "a corner on a side",
            bar(-1.0, -1.0, 1.0, 1.0, Material::Inside)?
                .placed(std::f64::consts::FRAC_PI_4, [2f64.sqrt(), 0.0]),
        ),
    ] {
        for reach in [0.25, 3.0] {
            let distance = found(&left, &other, reach)?;
            assert!(distance.abs() < 1e-9, "{name}, reach {reach}: {distance}");
            assert!(
                found(&other, &left, reach)?.abs() < 1e-9,
                "{name}, reach {reach}, the other way round"
            );
        }
    }
    // Drawn apart by a hair they have a hair between them; pushed together by a hair they overlap
    // by it.
    let apart = bar(0.0001, -40.0, 3.0, 40.0, Material::Inside)?;
    assert!((found(&left, &apart, 0.25)? - 0.0001).abs() < 1e-12);
    let pushed = bar(-0.0001, -40.0, 3.0, 40.0, Material::Inside)?;
    assert!((found(&left, &pushed, 0.25)? + 0.0001).abs() < 1e-12);
    Ok(())
}

#[test]
fn a_part_lying_in_the_material_of_the_other_overlaps() -> crate::Result<()> {
    // A peg beside its hole instead of in it: every point of the peg is in the plate.
    let peg = bar(-1.0, -1.0, 1.0, 1.0, Material::Inside)?;
    let hole = bar(-1.1, -1.1, 1.1, 1.1, Material::Outside)?;
    assert!((found(&peg, &hole, 0.5)? - 0.1).abs() < 1e-12); // in place: 0.1 all round
    for offset in [1.5, 2.3, 2.5] {
        let beside = found(&peg.placed(0.0, [offset, 0.0]), &hole, 0.5)?;
        assert!(beside < 0.0, "offset {offset}: {beside}");
    }
    // A long thin peg lying across the plate, far from the hole's corners, with no corner within
    // reach.
    let strip = bar(1.5, -60.0, 1.8, 60.0, Material::Inside)?;
    let across = found(&strip, &hole, 0.5)?;
    // Its near side is 0.4 from the hole's edge, in the plate
    assert!((across + 0.4).abs() < 1e-9, "{across}");
    // Farther from the hole's edge than the
    // reach: nothing is found, which is what the
    // reach means.
    assert_eq!(
        gap(&bar(30.0, -1.0, 32.0, 1.0, Material::Inside)?, &hole, 0.5)?,
        None
    );
    Ok(())
}
