//! The measurement itself, checked against the slow way: every point of each part against every
//! chord of the other. Parts with long straight pieces among short ones are the case the cells of
//! the search must not get wrong.

use crate::math::det;
use std::f64::consts::TAU;

use super::common::Dice;
use crate::teeth::mesh::{gap, Body, Material};
use crate::teeth::Point;

/// The distance from a point to a chord.
fn to_chord(p: Point, a: Point, b: Point) -> f64 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let t = (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
    det::hypot(p[0] - a[0] - t * ex, p[1] - a[1] - t * ey)
}

/// The smallest distance between two closed chains that do not overlap, the slow way.
fn slowly(a: &[Point], b: &[Point]) -> f64 {
    let one_way = |from: &[Point], onto: &[Point]| {
        from.iter()
            .flat_map(|&p| {
                onto.iter()
                    .zip(onto.iter().cycle().skip(1))
                    .map(move |(&u, &v)| to_chord(p, u, v))
            })
            .fold(f64::INFINITY, f64::min)
    };
    one_way(a, b).min(one_way(b, a))
}

/// A shape about `centre` that every ray from the centre meets once: `corners` points at uneven
/// angles (no two neighbours half a turn apart, so the centre stays inside and the chain does not
/// cross itself) and radii between `low` and `high`, so that long and short chords stand side by
/// side.
fn star(dice: &mut Dice, centre: Point, corners: usize, low: f64, high: f64) -> Vec<Point> {
    (0..corners)
        .map(|k| {
            let uneven = 0.4 * (dice.roll() % 1000) as f64 / 1000.0;
            let angle = TAU * (k as f64 + uneven) / corners as f64;
            let radius = low + (high - low) * (dice.roll() % 1000) as f64 / 1000.0;
            [
                centre[0] + radius * det::cos(angle),
                centre[1] + radius * det::sin(angle),
            ]
        })
        .collect()
}

#[test]
fn the_gap_is_the_gap_found_the_slow_way() -> crate::Result<()> {
    let mut dice = Dice(0x6A9);
    for round in 0..300 {
        // One part of few long chords or many short ones, the other likewise, apart by a random
        // distance.
        let corners = [3, 4, 5, 8, 40, 400];
        let a = star(&mut dice, [0.0, 0.0], corners[round % 6], 6.0, 10.0);
        let apart = 20.05 + (dice.roll() % 3000) as f64 / 1000.0;
        let direction = (dice.roll() % 628) as f64 / 100.0;
        let b = star(
            &mut dice,
            [apart * det::cos(direction), apart * det::sin(direction)],
            corners[(round / 6) % 6],
            6.0,
            10.0,
        );
        let (body_a, body_b) = (
            Body::new(a.clone(), Material::Inside)?,
            Body::new(b.clone(), Material::Inside)?,
        );
        let expected = slowly(&a, &b);
        for reach in [expected * 1.0001, expected + 0.5, 50.0] {
            let found = gap(&body_a, &body_b, reach)?.expect("within reach");
            assert!(
                (found.distance - expected).abs() < 1e-12,
                "round {round}, reach {reach}: {} for {expected}",
                found.distance
            );
        }
        assert_eq!(
            gap(&body_a, &body_b, expected * 0.999)?,
            None,
            "round {round}: found beyond the reach"
        );
        // With the second part's material outside its outline, the first part lies wholly in that
        // material: every point within reach counts as an overlap, the nearest of them by the gap
        // found before.
        let hollow = Body::new(b, Material::Outside)?;
        let just_reached = gap(&body_a, &hollow, expected * 1.0001)?.expect("within reach");
        assert!(
            just_reached.distance < 0.0
                && (just_reached.distance + expected).abs() < expected * 2e-4,
            "round {round}: {}",
            just_reached.distance
        );
    }
    Ok(())
}

#[test]
fn a_corner_at_a_repeated_point_is_judged_by_the_chords_on_either_side() -> crate::Result<()> {
    // A thin counter-clockwise triangle whose tip is given twice: the chord between the two is
    // of no length. Above the tip, apart from it, a small square.
    let triangle = Body::new(
        vec![[-1.0, -0.01], [0.0, 0.0], [0.0, 0.0], [-1.0, 0.01]],
        Material::Inside,
    )?;
    let square = Body::new(
        vec![
            [0.009, 0.099],
            [0.011, 0.099],
            [0.011, 0.101],
            [0.009, 0.101],
        ],
        Material::Inside,
    )?;
    // The parts do not overlap: the square's nearest corner is this far from the tip.
    let expected = det::hypot(0.009_f64, 0.099);
    for (a, b) in [(&triangle, &square), (&square, &triangle)] {
        let found = gap(a, b, 0.2)?.expect("within reach");
        assert!(
            found.distance > 0.0 && (found.distance - expected).abs() < 1e-12,
            "{} for {expected}",
            found.distance
        );
    }
    Ok(())
}

#[test]
fn a_long_bar_beside_a_fine_wheel_is_measured_rightly() -> crate::Result<()> {
    // A bar of four chords, one of them 400 mm long, 0.25 mm from a circle of 20,000 chords.
    let bar = Body::new(
        vec![
            [10.25, -200.0],
            [12.0, -200.0],
            [12.0, 200.0],
            [10.25, 200.0],
        ],
        Material::Inside,
    )?;
    let wheel: Vec<Point> = (0..20_000)
        .map(|i| TAU * f64::from(i) / 20_000.0)
        .map(|a| [10.0 * det::cos(a), 10.0 * det::sin(a)])
        .collect();
    let wheel = Body::new(wheel, Material::Inside)?;
    for step in 0..200 {
        // The wheel's nearest corner is at most half a chord off the line of centres: 1.3e-7 mm
        // farther away.
        let found = gap(&wheel.placed(f64::from(step) * 0.01, [0.0, 0.0]), &bar, 0.5)?
            .expect("within reach");
        assert!(
            found.distance >= 0.25 - 1e-12 && found.distance < 0.25 + 1.3e-7,
            "{}",
            found.distance
        );
    }
    // The same pushed 0.3 mm into the bar overlaps by 0.05.
    let pushed = gap(&wheel.placed(0.0, [0.3, 0.0]), &bar, 0.5)?.expect("overlapping");
    assert!(
        (pushed.distance + 0.05).abs() < 1.3e-7,
        "{}",
        pushed.distance
    );
    Ok(())
}
