//! Pairs of involute parts, checked by placing them and measuring the gap: a splined shaft in its
//! hub.

use std::f64::consts::TAU;

use crate::teeth::involute::{hub_for, External, Fit};
use crate::teeth::mesh::{gap, Body, Gap, Material};

/// How far the chords of the outlines may be from the curves: the gaps below are good to a few
/// times this.
const TOLERANCE: f64 = 1e-7;

fn body(e: &External) -> crate::Result<Body> {
    Body::new(e.outline(TOLERANCE)?.points(), Material::Inside)
}

// -------------------------------------------------------------------------------------------------
// A splined shaft in its hub
// -------------------------------------------------------------------------------------------------

fn shaft() -> External {
    External {
        module: 1.5,
        teeth: 20,
        pressure_angle: 30.0,
        addendum: 0.5,
        dedendum: 0.85,
        shift: 0.0,
        tool_tip_radius: 0.4,
        thinning: 0.0,
    }
}

fn fit(side_clearance: f64) -> Fit {
    Fit {
        side_clearance,
        hub_addendum: 0.5,
        tip_clearance: 0.2,
        hub_fillet_radius: 0.3,
        form_clearance: 0.05,
    }
}

fn shaft_in_hub(side_clearance: f64, turned: f64) -> crate::Result<Option<Gap>> {
    let shaft = shaft();
    let hub = hub_for(&shaft, &fit(side_clearance))?;
    let hub_body = Body::new(hub.outline(TOLERANCE)?.points(), Material::Outside)?;
    gap(&body(&shaft)?.placed(turned, [0.0, 0.0]), &hub_body, 0.5)
}

#[test]
fn a_shaft_sits_in_its_hub_with_the_clearance_asked_for() -> crate::Result<()> {
    // Between flanks the play asked for on the pitch circle appears turned by the pressure angle,
    // half on each side.
    for side in [0.06, 0.02, 0.2] {
        let each_side = side * 30f64.to_radians().cos() / 2.0;
        let found = shaft_in_hub(side, 0.0)?.expect("within reach");
        assert!(
            (found.distance - each_side).abs() < 2e-6,
            "gap {} for {each_side}",
            found.distance
        );
    }
    // No clearance: the flanks touch on both sides and nothing overlaps.
    let tight = shaft_in_hub(0.0, 0.0)?.expect("touching");
    assert!(tight.distance.abs() < 2e-6, "{}", tight.distance);
    Ok(())
}

#[test]
fn a_shaft_turns_in_its_hub_by_half_the_play_and_no_further() -> crate::Result<()> {
    let (side, pitch_radius) = (0.06, shaft().pitch_radius()?);
    let half_play = side / 2.0 / pitch_radius; // the angle
    let touching = shaft_in_hub(side, half_play)?.expect("touching");
    assert!(touching.distance.abs() < 2e-6, "{}", touching.distance);
    let free = shaft_in_hub(side, 0.5 * half_play)?.expect("within reach");
    assert!(free.distance > 0.005);
    let jammed = shaft_in_hub(side, 1.5 * half_play)?.expect("overlapping");
    assert!(jammed.distance < -0.005, "{}", jammed.distance);
    // Turned by a whole pitch it is the same fit again.
    let next = shaft_in_hub(side, TAU / 20.0)?.expect("within reach");
    assert!((next.distance - side * 30f64.to_radians().cos() / 2.0).abs() < 2e-6);
    Ok(())
}

#[test]
fn bodies_and_reach_are_checked() -> crate::Result<()> {
    let square = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let clockwise: Vec<[f64; 2]> = square.iter().rev().copied().collect();
    assert!(Body::new(clockwise, Material::Inside).is_err());
    assert!(Body::new(vec![[0.0, 0.0], [1.0, 0.0]], Material::Inside).is_err());
    assert!(Body::new(
        vec![[0.0, 0.0], [1.0, f64::NAN], [0.0, 1.0]],
        Material::Inside
    )
    .is_err());
    let a = Body::new(square.clone(), Material::Inside)?;
    assert!(gap(&a, &a, 0.0).is_err() && gap(&a, &a, f64::NAN).is_err());
    // Two squares a quarter apart, corner regions included; then overlapping; then out of reach.
    let apart = gap(&a, &a.placed(0.0, [1.25, 0.0]), 0.5)?.expect("within reach");
    assert!((apart.distance - 0.25).abs() < 1e-12);
    let diagonal = gap(&a, &a.placed(0.0, [1.3, 1.4]), 1.0)?.expect("within reach");
    assert!((diagonal.distance - 0.5).abs() < 1e-12);
    let overlapping = gap(&a, &a.placed(0.0, [0.9, 0.2]), 0.5)?.expect("overlapping");
    assert!(
        (overlapping.distance + 0.1).abs() < 1e-12,
        "{}",
        overlapping.distance
    );
    assert_eq!(gap(&a, &a.placed(0.0, [3.0, 0.0]), 0.5)?, None);
    // A square in a square hole, 0.1 clear all round.
    let hole = Body::new(
        vec![[-0.1, -0.1], [1.1, -0.1], [1.1, 1.1], [-0.1, 1.1]],
        Material::Outside,
    )?;
    assert!((gap(&a, &hole, 0.5)?.expect("within reach").distance - 0.1).abs() < 1e-12);
    assert!(
        (gap(&a.placed(0.0, [0.15, 0.0]), &hole, 0.5)?
            .expect("overlapping")
            .distance
            + 0.05)
            .abs()
            < 1e-12
    );
    Ok(())
}
