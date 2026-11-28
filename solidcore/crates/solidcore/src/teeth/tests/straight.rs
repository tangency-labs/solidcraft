//! Straight-sided splines and serrations: every piece exact, every piece where its construction
//! puts it, the hub at the clearance asked for all round, and the turning play the same as the play
//! found by turning the hub.

use std::f64::consts::{PI, TAU};

use super::common::{check_outline, dist, sound, HOSTILE, HOSTILE_COUNTS};
use crate::error::teeth::Why;
use crate::teeth::mesh::{gap, Body};
use crate::teeth::straight::{
    hub_for, serration_hub_for, serration_pair, serration_play, spline_pair, turning_play, Fit,
    Serration, StraightHub, StraightSpline, TipCorner,
};
use crate::teeth::{Outline, Pair, Piece, Point, Role};

/// How far the chords of the outlines may be from the curves: the gaps below are good to a few
/// times this.
const TOLERANCE: f64 = 1e-7;

fn spline(tip: TipCorner, root_round: f64) -> StraightSpline {
    StraightSpline {
        teeth: 6,
        major_diameter: 28.0,
        minor_diameter: 23.0,
        tooth_width: 6.0,
        root_round,
        tip,
    }
}

fn fit(side: f64, major: f64, minor: f64) -> Fit {
    Fit {
        side_clearance: side,
        major_clearance: major,
        minor_clearance: minor,
        hub_root_round: 0.2,
        hub_tip_round: 0.3,
    }
}

fn splines() -> Vec<(&'static str, StraightSpline)> {
    vec![
        ("sharp", spline(TipCorner::Plain, 0.0)),
        (
            "chamfered, rounded roots",
            spline(TipCorner::Chamfer { size: 0.3 }, 0.3),
        ),
        (
            "rounded tips and roots",
            spline(TipCorner::Round { radius: 0.4 }, 0.3),
        ),
        (
            "ten narrow teeth",
            StraightSpline {
                teeth: 10,
                major_diameter: 52.0,
                minor_diameter: 46.0,
                tooth_width: 5.0,
                root_round: 0.4,
                tip: TipCorner::Chamfer { size: 0.4 },
            },
        ),
    ]
}

fn serration(tip: TipCorner, root_round: f64) -> Serration {
    Serration {
        teeth: 36,
        major_diameter: 20.0,
        minor_diameter: 18.8,
        flank_angle: 90.0,
        tip,
        root_round,
    }
}

/// Serrated shafts, each with the rounds of its hub's corners.
fn serrations() -> Vec<(&'static str, Serration, [f64; 2])> {
    vec![
        (
            "90 degrees, flats",
            serration(TipCorner::Plain, 0.0),
            [0.0, 0.0],
        ),
        (
            "90 degrees, rounds",
            serration(TipCorner::Round { radius: 0.1 }, 0.03),
            [0.03, 0.05],
        ),
        (
            "60 degrees, chamfered",
            Serration {
                teeth: 24,
                major_diameter: 30.0,
                minor_diameter: 28.2,
                flank_angle: 60.0,
                tip: TipCorner::Chamfer { size: 0.08 },
                root_round: 0.1,
            },
            [0.05, 0.15],
        ),
    ]
}

fn serration_fit(
    side: f64,
    major: f64,
    minor: f64,
    [hub_root_round, hub_tip_round]: [f64; 2],
) -> Fit {
    Fit {
        side_clearance: side,
        major_clearance: major,
        minor_clearance: minor,
        hub_root_round,
        hub_tip_round,
    }
}

/// Asserts that every piece of an outline is an exact line or arc.
fn all_exact(name: &str, outline: &Outline) {
    for piece in outline.tooth() {
        assert!(
            matches!(piece, Piece::Line { .. } | Piece::Arc { .. }),
            "{name}: {piece:?} is not exact"
        );
    }
}

/// The distance of a point from a line through `on` with unit normal `normal`, signed along the
/// normal.
fn off_line(p: Point, on_level: f64, normal: Point) -> f64 {
    p[0] * normal[0] + p[1] * normal[1] - on_level
}

/// Checks each piece of a shaft's tooth against the construction: flanks on the flank line (or its
/// mirror), tips and roots on their circles, rounds that touch the flank and the circle, chamfers
/// at 45 degrees.
///
/// `normal` and `level` give the upper flank line; the lower is its mirror in the x axis.
fn on_construction(
    name: &str,
    outline: &Outline,
    normal: Point,
    level: f64,
    major: f64,
    minor: f64,
) {
    let on_flank = |p: Point| {
        off_line(p, level, normal).abs() < 1e-9
            || off_line([p[0], -p[1]], level, normal).abs() < 1e-9
    };
    let pitch = outline.pitch_angle();
    for piece in outline.tooth() {
        let (start, end) = (
            piece.start().expect("a start"),
            piece.end().expect("an end"),
        );
        match (piece, piece.role()) {
            (Piece::Line { .. }, Role::Flank) => {
                assert!(
                    on_flank(start) && on_flank(end),
                    "{name}: a flank off the flank line"
                );
            }
            (Piece::Arc { centre, radius, .. }, Role::Tip) if dist(*centre, [0.0, 0.0]) < 1e-12 => {
                assert!(
                    (radius - major).abs() < 1e-12,
                    "{name}: tip radius {radius}"
                );
            }
            (Piece::Arc { centre, radius, .. }, Role::Root) => {
                assert!(
                    dist(*centre, [0.0, 0.0]) < 1e-12 && (radius - minor).abs() < 1e-12,
                    "{name}: root"
                );
            }
            (Piece::Arc { centre, radius, .. }, Role::Tip) => {
                // A tip round touches the major circle from inside and the flank from the tooth's
                // side.
                assert!(
                    (dist(*centre, [0.0, 0.0]) + radius - major).abs() < 1e-9,
                    "{name}: tip round off the circle"
                );
                let side = [
                    off_line(*centre, level, normal),
                    off_line([centre[0], -centre[1]], level, normal),
                ]
                .into_iter()
                .fold(f64::INFINITY, |a, b| {
                    if (b + radius).abs() < (a + radius).abs() {
                        b
                    } else {
                        a
                    }
                });
                assert!(
                    (side + radius).abs() < 1e-9,
                    "{name}: tip round does not touch the flank ({side})"
                );
                assert!(
                    on_flank(start) || on_flank(end),
                    "{name}: tip round does not end on the flank"
                );
            }
            (Piece::Arc { centre, radius, .. }, Role::Fillet) => {
                // A root round touches the minor circle from outside and a flank from the space's
                // side, where the flank may be the next tooth's, a pitch on.
                assert!(
                    (dist(*centre, [0.0, 0.0]) - radius - minor).abs() < 1e-9,
                    "{name}: root round off the circle"
                );
                let back = crate::teeth::outline::rotated(*centre, -pitch);
                let side = [*centre, back]
                    .into_iter()
                    .flat_map(|c| {
                        [
                            off_line(c, level, normal),
                            off_line([c[0], -c[1]], level, normal),
                        ]
                    })
                    .fold(f64::INFINITY, |a, b| {
                        if (b - radius).abs() < (a - radius).abs() {
                            b
                        } else {
                            a
                        }
                    });
                assert!(
                    (side - radius).abs() < 1e-9,
                    "{name}: root round does not touch the flank ({side})"
                );
            }
            (Piece::Line { .. }, Role::Tip) => {
                // A chamfer from the flank to the major circle, at 45 degrees to the flank.
                let (on_circle, on_line) = if on_flank(end) {
                    (start, end)
                } else {
                    (end, start)
                };
                assert!(
                    (dist(on_circle, [0.0, 0.0]) - major).abs() < 1e-9,
                    "{name}: chamfer off the circle"
                );
                assert!(
                    on_flank(on_line),
                    "{name}: chamfer does not reach the flank"
                );
                let way = [on_circle[0] - on_line[0], on_circle[1] - on_line[1]];
                let upper = off_line(on_line, level, normal).abs() < 1e-9;
                let flank_way = if upper {
                    [-normal[1], normal[0]]
                } else {
                    [-normal[1], -normal[0]]
                };
                let cos =
                    (way[0] * flank_way[0] + way[1] * flank_way[1]).abs() / way[0].hypot(way[1]);
                assert!(
                    (cos - 0.5f64.sqrt()).abs() < 1e-9,
                    "{name}: chamfer not at 45 degrees"
                );
            }
            other => panic!("{name}: a piece that should not be there: {other:?}"),
        }
    }
}

#[test]
fn every_piece_of_a_spline_is_exact_and_where_its_construction_puts_it() -> crate::Result<()> {
    for (name, shaft) in splines() {
        let outline = shaft.outline(1e-3)?;
        check_outline(name, &outline);
        all_exact(name, &outline);
        assert_eq!(outline.teeth(), shaft.teeth);
        let (major, minor) = (shaft.major_diameter / 2.0, shaft.minor_diameter / 2.0);
        on_construction(
            name,
            &outline,
            [0.0, 1.0],
            shaft.tooth_width / 2.0,
            major,
            minor,
        );
        let (low, high) = outline.radii();
        assert!(
            (low - minor).abs() < 1e-12 && (high - major).abs() < 1e-12,
            "{name}: radii {low} {high}"
        );
        // The hub's opening is exact as well, with its flanks at half the space width.
        let hub = hub_for(&shaft, &fit(0.05, 0.3, 0.2))?;
        let opening = hub.outline(1e-3)?;
        check_outline(name, &opening);
        all_exact(name, &opening);
        assert!((hub.space_width - shaft.tooth_width - 0.1).abs() < 1e-12);
        assert!((hub.major_diameter - shaft.major_diameter - 0.6).abs() < 1e-12);
        assert!((hub.minor_diameter - shaft.minor_diameter - 0.4).abs() < 1e-12);
    }
    // The count of pieces: sharp corners give flank, tip, flank, root; each round or chamfer adds
    // one.
    assert_eq!(
        spline(TipCorner::Plain, 0.0).outline(1e-3)?.tooth().len(),
        4
    );
    assert_eq!(
        spline(TipCorner::Chamfer { size: 0.3 }, 0.3)
            .outline(1e-3)?
            .tooth()
            .len(),
        8
    );
    Ok(())
}

#[test]
fn every_piece_of_a_serration_is_exact_and_where_its_construction_puts_it() -> crate::Result<()> {
    for (name, shaft, rounds) in serrations() {
        let outline = shaft.outline(1e-3)?;
        check_outline(name, &outline);
        all_exact(name, &outline);
        let half = shaft.flank_angle.to_radians() / 2.0;
        let normal = [half.sin(), half.cos()];
        // The flank crosses the mean circle a quarter of a pitch from the middle of the tooth.
        let mean = (shaft.major_diameter + shaft.minor_diameter) / 4.0;
        let quarter = PI / (2.0 * f64::from(shaft.teeth));
        let level = normal[0] * mean * quarter.cos() + normal[1] * mean * quarter.sin();
        on_construction(
            name,
            &outline,
            normal,
            level,
            shaft.major_diameter / 2.0,
            shaft.minor_diameter / 2.0,
        );
        // Where the flanks would meet: on the x axis, on both flank lines.
        let apex = shaft.sharp_tip_radius()?;
        assert!(
            off_line([apex, 0.0], level, normal).abs() < 1e-12 && apex > shaft.major_diameter / 2.0
        );
        let opening =
            serration_hub_for(&shaft, &serration_fit(0.02, 0.05, 0.05, rounds))?.outline(1e-3)?;
        check_outline(name, &opening);
        all_exact(name, &opening);
    }
    Ok(())
}

/// The two parts of a pair as bodies, with every chord longer than a fiftieth of a millimetre cut
/// into shorter ones on the same line: the same outlines, but quicker to measure, since the
/// measurement sorts chords into cells no smaller than the longest chord.
fn fine_bodies(pair: &Pair) -> crate::Result<(Body, Body)> {
    let fine = |points: Vec<Point>| -> Vec<Point> {
        let mut out = Vec::with_capacity(points.len());
        for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
            let cuts = (dist(*a, *b) / 0.02).ceil().clamp(1.0, 10_000.0) as usize;
            out.extend((0..cuts).map(|i| {
                let t = i as f64 / cuts as f64;
                [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
            }));
        }
        out
    };
    Ok((
        Body::new(fine(pair.first.outline.points()), pair.first.material)?,
        Body::new(fine(pair.second.outline.points()), pair.second.material)?,
    ))
}

/// The smallest gap of a pair at one position, the hub turned on the shaft by `extra` and its axis
/// moved by `moved`, measured on the bodies of [`fine_bodies`].
fn gap_of(
    bodies: &(Body, Body),
    pair: &Pair,
    extra: f64,
    moved: Point,
    reach: f64,
) -> crate::Result<Option<f64>> {
    let placed = bodies.1.placed(pair.second_start + extra, moved);
    Ok(gap(&bodies.0, &placed, reach)?.map(|g| g.distance))
}

/// The turn of the hub on its shaft at which the gap closes, found by trying: turning further until
/// the parts overlap, then halving between a turn with a gap and a turn with an overlap.
fn play_by_trying(bodies: &(Body, Body), pair: &Pair, reach: f64) -> crate::Result<f64> {
    let gap = |turn: f64| -> crate::Result<f64> {
        Ok(gap_of(bodies, pair, turn, [0.0, 0.0], reach)?.unwrap_or(f64::INFINITY))
    };
    let (mut free, mut stuck) = (0.0, pair.cycle / 256.0);
    for _ in 0..8 {
        if gap(stuck)? < 0.0 {
            break;
        }
        (free, stuck) = (stuck, 2.0 * stuck);
    }
    assert!(
        gap(stuck)? < 0.0,
        "turned far enough, the hub must overlap the shaft"
    );
    for _ in 0..30 {
        let middle = 0.5 * (free + stuck);
        if gap(middle)? > 0.0 {
            free = middle;
        } else {
            stuck = middle;
        }
    }
    Ok(0.5 * (free + stuck))
}

#[test]
fn a_spline_sits_in_its_hub_with_the_smallest_clearance_all_round() -> crate::Result<()> {
    let shaft = spline(TipCorner::Chamfer { size: 0.3 }, 0.3);
    for (side, major, minor) in [
        (0.05, 0.3, 0.2),
        (0.0, 0.3, 0.2),
        (0.2, 0.08, 0.3),
        (0.1, 0.25, 0.03),
        (0.0, 0.0, 0.0),
    ] {
        let pair = spline_pair(&shaft, &fit(side, major, minor), TOLERANCE)?;
        let bodies = fine_bodies(&pair)?;
        let least = side.min(major).min(minor);
        super::common::assert_gap("spline", &pair, 2, least + 0.05, least, 2e-6)?;
        // Moved off the axis by more than the clearance, the hub reaches into the shaft.
        let off = gap_of(&bodies, &pair, 0.0, [least + 0.02, 0.0], 0.05)?.expect("overlapping");
        assert!(off < -0.005, "moved by {}: gap {off}", least + 0.02);
    }
    // The other shapes of tooth, with one fit.
    for (name, shaft) in splines() {
        let pair = spline_pair(&shaft, &fit(0.05, 0.3, 0.2), TOLERANCE)?;
        super::common::assert_gap(name, &pair, 2, 0.1, 0.05, 2e-6)?;
    }
    Ok(())
}

#[test]
fn a_spline_hub_turns_by_its_play_and_no_further() -> crate::Result<()> {
    for (name, shaft) in [
        ("chamfered", spline(TipCorner::Chamfer { size: 0.3 }, 0.3)),
        ("rounded", splines()[2].1),
    ] {
        for side in [0.05, 0.15] {
            let fit = fit(side, 0.3, 0.2);
            let play = turning_play(&shaft, &fit)?;
            let pair = spline_pair(&shaft, &fit, TOLERANCE)?;
            let bodies = fine_bodies(&pair)?;
            let tried = play_by_trying(&bodies, &pair, 0.05)?;
            assert!(
                (tried - play).abs() < 2e-7,
                "{name}, {side}: play {play}, found by turning {tried}"
            );
            let touching = gap_of(&bodies, &pair, play, [0.0, 0.0], 0.05)?.expect("touching");
            assert!(touching.abs() < 2e-6, "{name}: gap {touching} at the play");
            let beyond =
                gap_of(&bodies, &pair, 1.2 * play, [0.0, 0.0], 0.05)?.expect("overlapping");
            assert!(beyond < -1e-4, "{name}: gap {beyond} beyond the play");
            // The other way is the same, by symmetry.
            let back = gap_of(&bodies, &pair, -play, [0.0, 0.0], 0.05)?.expect("touching");
            assert!(
                back.abs() < 2e-6,
                "{name}: gap {back} at the play the other way"
            );
        }
    }
    // With no side clearance there is no play.
    assert!(turning_play(&spline(TipCorner::Plain, 0.0), &fit(0.0, 0.3, 0.2))?.abs() < 1e-12);
    Ok(())
}

#[test]
fn a_serrated_shaft_sits_in_its_hub_with_the_smallest_clearance_and_turns_by_its_play(
) -> crate::Result<()> {
    for (name, shaft, rounds) in serrations() {
        for (side, major, minor) in [
            (0.02, 0.05, 0.05),
            (0.0, 0.05, 0.04),
            (0.04, 0.03, 0.06),
            (0.0, 0.0, 0.0),
        ] {
            let fit = serration_fit(side, major, minor, rounds);
            let pair = serration_pair(&shaft, &fit, TOLERANCE)?;
            let bodies = fine_bodies(&pair)?;
            super::common::assert_gap(
                name,
                &pair,
                2,
                0.05 + side.min(major).min(minor),
                side.min(major).min(minor),
                2e-6,
            )?;
            let off = gap_of(
                &bodies,
                &pair,
                0.0,
                [side.min(major).min(minor) + 0.01, 0.0],
                0.05,
            )?
            .expect("overlapping");
            assert!(off < -0.003, "{name}: moved, gap {off}");
        }
        let fit = serration_fit(0.02, 0.05, 0.05, rounds);
        let play = serration_play(&shaft, &fit)?;
        let pair = serration_pair(&shaft, &fit, TOLERANCE)?;
        let bodies = fine_bodies(&pair)?;
        let tried = play_by_trying(&bodies, &pair, 0.05)?;
        assert!(
            (tried - play).abs() < 1e-6 * play.max(1e-3),
            "{name}: play {play}, found by turning {tried}"
        );
        assert!(
            gap_of(&bodies, &pair, play, [0.0, 0.0], 0.05)?
                .expect("touching")
                .abs()
                < 2e-6
        );
        assert!(
            gap_of(&bodies, &pair, 1.2 * play, [0.0, 0.0], 0.05)?.expect("overlapping") < -1e-4
        );
    }
    Ok(())
}

#[test]
fn what_cannot_be_made_is_refused() {
    let good = spline(TipCorner::Chamfer { size: 0.3 }, 0.3);
    let refused = |shaft: StraightSpline| shaft.outline(1e-3).is_err();
    // Teeth so wide that neighbours meet at the root, wider than the minor circle, rounds that do
    // not fit.
    assert!(refused(StraightSpline {
        teeth: 12,
        tooth_width: 6.5,
        ..good
    }));
    assert!(refused(StraightSpline {
        tooth_width: 24.0,
        ..good
    }));
    assert!(refused(StraightSpline {
        tip: TipCorner::Round { radius: 3.5 },
        ..good
    }));
    assert!(refused(StraightSpline {
        tip: TipCorner::Chamfer { size: 4.0 },
        ..good
    }));
    assert!(refused(StraightSpline {
        root_round: 5.0,
        ..good
    }));
    assert!(refused(StraightSpline {
        minor_diameter: 28.0,
        ..good
    }));
    assert!(refused(StraightSpline { teeth: 1, ..good }));
    // Rounds at the corners of the hub's spaces that would come closer to the shaft than the
    // clearance.
    let close = |hub_root_round: f64, hub_tip_round: f64| Fit {
        hub_root_round,
        hub_tip_round,
        ..fit(0.05, 0.3, 0.2)
    };
    let why = |f: Fit| {
        let found = hub_for(&good, &f);
        super::common::why(&found).unwrap_or_else(|| panic!("not refused for a reason: {found:?}"))
    };
    assert_eq!(why(close(1.0, 0.3)), Why::HubRootRoundsTooClose);
    assert_eq!(
        why(close(0.2, 0.0)),
        Why::HubTipsTooClose,
        "{:?}",
        why(close(0.2, 0.0))
    );
    // The largest hub root round that fits keeps exactly the clearance; a little more does not fit.
    assert!(hub_for(&good, &close(0.2, 0.3)).is_ok());
    // A minor clearance that takes the hub's teeth past its roots.
    assert!(hub_for(&good, &fit(0.05, 0.3, 3.0)).is_err());
    // A serration whose flanks would meet below the major circle, or above the minor circle.
    let s = serration(TipCorner::Plain, 0.0);
    assert!(Serration {
        flank_angle: 150.0,
        ..s
    }
    .outline(1e-3)
    .is_err());
    assert!(Serration { teeth: 200, ..s }.outline(1e-3).is_err());
}

#[test]
fn the_refusal_of_hub_rounds_is_where_the_measured_gap_falls_below_the_clearance(
) -> crate::Result<()> {
    // The largest hub root round accepted, found by halving between accepted and refused.
    let shaft = spline(TipCorner::Round { radius: 0.4 }, 0.3);
    let base = fit(0.05, 0.3, 0.2);
    let accepted = |radius: f64| {
        hub_for(
            &shaft,
            &Fit {
                hub_root_round: radius,
                ..base
            },
        )
        .is_ok()
    };
    let (mut good, mut bad) = (0.0, 2.9);
    assert!(accepted(good) && !accepted(bad));
    for _ in 0..40 {
        let middle = 0.5 * (good + bad);
        if accepted(middle) {
            good = middle;
        } else {
            bad = middle;
        }
    }
    assert!(good > 0.4, "{good}");
    // At the largest accepted round the gap measured is still the clearance; a hub made with a
    // round a little larger, by hand, comes closer than the clearance at its corners.
    let hub = hub_for(
        &shaft,
        &Fit {
            hub_root_round: good,
            ..base
        },
    )?;
    let measured = |hub: &StraightHub| -> crate::Result<f64> {
        let pair = Pair {
            second: crate::teeth::Part {
                outline: hub.outline(TOLERANCE)?,
                ..spline_pair(&shaft, &base, 1e-3)?.second
            },
            ..spline_pair(&shaft, &base, TOLERANCE)?
        };
        Ok(super::common::least(&pair.through_a_cycle(1, 0.2)?))
    };
    let at_limit = measured(&hub)?;
    assert!(
        (at_limit - 0.05).abs() < 2e-6,
        "at the limit: least gap {at_limit}"
    );
    let beyond = measured(&StraightHub {
        root_round: good + 0.05,
        ..hub
    })?;
    assert!(beyond < 0.05 - 1e-3, "beyond the limit: least gap {beyond}");
    // The same at the tips of the hub's teeth, against the shaft's root rounds: the smallest
    // accepted round.
    let accepted = |radius: f64| {
        hub_for(
            &shaft,
            &Fit {
                hub_tip_round: radius,
                ..base
            },
        )
        .is_ok()
    };
    let (mut bad, mut good) = (0.0, 0.3);
    assert!(accepted(good) && !accepted(bad));
    for _ in 0..40 {
        let middle = 0.5 * (good + bad);
        if accepted(middle) {
            good = middle;
        } else {
            bad = middle;
        }
    }
    let hub = hub_for(
        &shaft,
        &Fit {
            hub_tip_round: good,
            ..base
        },
    )?;
    let at_limit = measured(&hub)?;
    assert!(
        (at_limit - 0.05).abs() < 2e-6,
        "at the limit: least gap {at_limit}"
    );
    let beyond = measured(&StraightHub {
        tip_round: (good - 0.03).max(0.0),
        ..hub
    })?;
    assert!(beyond < 0.05 - 1e-3, "beyond the limit: least gap {beyond}");
    Ok(())
}

#[test]
fn hostile_numbers_never_panic() {
    let mut dice = super::common::Dice(7);
    let tips = |dice: &mut super::common::Dice| match dice.roll() % 3 {
        0 => TipCorner::Plain,
        1 => TipCorner::Chamfer {
            size: dice.pick(&HOSTILE),
        },
        _ => TipCorner::Round {
            radius: dice.pick(&HOSTILE),
        },
    };
    for _ in 0..3000 {
        let shaft = StraightSpline {
            teeth: dice.pick(&HOSTILE_COUNTS),
            major_diameter: dice.pick(&HOSTILE),
            minor_diameter: dice.pick(&HOSTILE),
            tooth_width: dice.pick(&HOSTILE),
            root_round: dice.pick(&HOSTILE),
            tip: tips(&mut dice),
        };
        let fit = Fit {
            side_clearance: dice.pick(&HOSTILE),
            major_clearance: dice.pick(&HOSTILE),
            minor_clearance: dice.pick(&HOSTILE),
            hub_root_round: dice.pick(&HOSTILE),
            hub_tip_round: dice.pick(&HOSTILE),
        };
        let tolerance = dice.pick(&[f64::NAN, 0.0, -1.0, 1e-300, 1e-3, 0.1, 1e9]);
        sound(shaft.outline(tolerance));
        if let Ok(hub) = hub_for(&shaft, &fit) {
            sound(hub.outline(tolerance));
        }
        let _ = turning_play(&shaft, &fit).map(|p| assert!(p.is_finite() && p >= 0.0));
        let _ = spline_pair(&shaft, &fit, tolerance);
        let hub = StraightHub {
            teeth: dice.pick(&HOSTILE_COUNTS),
            major_diameter: dice.pick(&HOSTILE),
            minor_diameter: dice.pick(&HOSTILE),
            space_width: dice.pick(&HOSTILE),
            root_round: dice.pick(&HOSTILE),
            tip_round: dice.pick(&HOSTILE),
        };
        sound(hub.outline(tolerance));
        let serrated = Serration {
            teeth: dice.pick(&HOSTILE_COUNTS),
            major_diameter: dice.pick(&HOSTILE),
            minor_diameter: dice.pick(&HOSTILE),
            flank_angle: dice.pick(&HOSTILE),
            tip: tips(&mut dice),
            root_round: dice.pick(&HOSTILE),
        };
        sound(serrated.outline(tolerance));
        let _ = serrated.sharp_tip_radius().map(|r| assert!(r.is_finite()));
        if let Ok(hub) = serration_hub_for(&serrated, &fit) {
            sound(hub.outline(tolerance));
            let _ = hub.major_diameter().map(|d| assert!(d.is_finite()));
            let _ = hub.minor_diameter().map(|d| assert!(d.is_finite()));
        }
        let _ = serration_play(&serrated, &fit).map(|p| assert!(p.is_finite() && p >= 0.0));
        let _ = serration_pair(&serrated, &fit, tolerance);
    }
}

#[test]
fn plausible_numbers_give_a_sound_spline_or_a_refusal() {
    let mut dice = super::common::Dice(11);
    let (mut made, mut fitted) = (0, 0);
    for _ in 0..1500 {
        let major = dice.pick(&[8.0, 16.0, 28.0, 50.0, 120.0]);
        let shaft = StraightSpline {
            teeth: dice.pick(&[2, 3, 4, 6, 8, 10, 16, 24]),
            major_diameter: major,
            minor_diameter: major * dice.pick(&[0.6, 0.8, 0.9, 0.97]),
            tooth_width: major * dice.pick(&[0.05, 0.1, 0.2, 0.3, 0.5]),
            root_round: major * dice.pick(&[0.0, 0.005, 0.01, 0.05]),
            tip: match dice.roll() % 3 {
                0 => TipCorner::Plain,
                1 => TipCorner::Chamfer {
                    size: major * dice.pick(&[0.002, 0.01, 0.05]),
                },
                _ => TipCorner::Round {
                    radius: major * dice.pick(&[0.002, 0.01, 0.05]),
                },
            },
        };
        let fit = Fit {
            side_clearance: dice.pick(&[0.0, 0.01, 0.05, 0.2]),
            major_clearance: dice.pick(&[0.0, 0.05, 0.3]),
            minor_clearance: dice.pick(&[0.0, 0.05, 0.3]),
            hub_root_round: major * dice.pick(&[0.0, 0.005, 0.02]),
            hub_tip_round: major * dice.pick(&[0.0, 0.005, 0.02]),
        };
        if sound(shaft.outline(1e-3)) {
            made += 1;
            if let Ok(hub) = hub_for(&shaft, &fit) {
                assert!(sound(hub.outline(1e-3)), "{shaft:?} {fit:?}");
                let play =
                    turning_play(&shaft, &fit).unwrap_or_else(|e| panic!("{shaft:?} {fit:?}: {e}"));
                assert!(play.is_finite() && play >= 0.0 && play < TAU / f64::from(shaft.teeth));
                fitted += 1;
            }
        }
    }
    assert!(made > 300 && fitted > 150, "{made} made, {fitted} fitted");
    // Serrations, the same way.
    let mut made = 0;
    for _ in 0..800 {
        let major = dice.pick(&[6.0, 12.0, 20.0, 40.0]);
        let shaft = Serration {
            teeth: dice.pick(&[12, 24, 36, 48, 72]),
            major_diameter: major,
            minor_diameter: major * dice.pick(&[0.9, 0.93, 0.96]),
            flank_angle: dice.pick(&[45.0, 60.0, 90.0, 120.0]),
            tip: if dice.roll().is_multiple_of(2) {
                TipCorner::Plain
            } else {
                TipCorner::Round {
                    radius: major * 0.003,
                }
            },
            root_round: major * dice.pick(&[0.0, 0.002, 0.004]),
        };
        if sound(shaft.outline(1e-3)) {
            made += 1;
            if serration_hub_for(&shaft, &serration_fit(0.01, 0.02, 0.02, [0.0, 0.0])).is_ok() {
                let play = serration_play(&shaft, &serration_fit(0.01, 0.02, 0.02, [0.0, 0.0]))
                    .unwrap_or_else(|e| panic!("{shaft:?}: {e}"));
                assert!(play.is_finite() && play > 0.0);
            }
        }
    }
    assert!(made > 100, "{made} serrations made");
}
