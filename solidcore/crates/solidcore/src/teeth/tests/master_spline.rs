//! Master splines: one tooth left out or one wide tooth, so that the hub goes on its shaft one way
//! only.

use crate::math::det;
use std::f64::consts::TAU;

use super::common::{check_outline, operation, out_of_range, why, HOSTILE};
use crate::error::teeth::{TeethFailure, Why};
use crate::teeth::involute::{self, External};
use crate::teeth::master_spline::{
    self, master_pair, master_spline, placement, replaced_period, Circle, Form, Placement,
};
use crate::teeth::straight::{self, Serration, StraightSpline, TipCorner};
use crate::teeth::{Material, Motion, Outline, Pair, Piece, Point, Role};

/// The three kinds of spline as the dialog's defaults make them: an involute shaft of 20 teeth, a
/// straight-sided one of 6 and a serration of 36, each with its hub, the tolerance its outlines are
/// made with and how far a gap is looked for.
fn default_pairs() -> Vec<(&'static str, Pair, f64, f64)> {
    let shaft = External {
        module: 1.5,
        teeth: 20,
        pressure_angle: 30.0,
        addendum: 0.5,
        dedendum: 0.85,
        shift: 0.0,
        tool_tip_radius: 0.4,
        thinning: 0.0,
    };
    let fit = involute::Fit {
        side_clearance: 0.06,
        hub_addendum: 0.5,
        tip_clearance: 0.2,
        hub_fillet_radius: 0.3,
        form_clearance: 0.05,
    };
    let spline = StraightSpline {
        teeth: 6,
        major_diameter: 28.0,
        minor_diameter: 23.0,
        tooth_width: 6.0,
        root_round: 0.3,
        tip: TipCorner::Chamfer { size: 0.3 },
    };
    let spline_fit = straight::Fit {
        side_clearance: 0.05,
        major_clearance: 0.3,
        minor_clearance: 0.2,
        hub_root_round: 0.2,
        hub_tip_round: 0.3,
    };
    let serration = Serration {
        teeth: 36,
        major_diameter: 20.0,
        minor_diameter: 18.8,
        flank_angle: 90.0,
        tip: TipCorner::Round { radius: 0.1 },
        root_round: 0.03,
    };
    let serration_fit = straight::Fit {
        side_clearance: 0.02,
        major_clearance: 0.05,
        minor_clearance: 0.05,
        hub_root_round: 0.03,
        hub_tip_round: 0.05,
    };
    let (a, b, c) = (1.35 * 1.5 / 600.0, 2.5 / 600.0, 0.6 / 600.0);
    vec![
        (
            "involute",
            involute::spline_pair(&shaft, &fit, a).expect("the involute pair"),
            a,
            2.5,
        ),
        (
            "straight",
            straight::spline_pair(&spline, &spline_fit, b).expect("the straight-sided pair"),
            b,
            3.0,
        ),
        (
            "serration",
            straight::serration_pair(&serration, &serration_fit, c).expect("the serrated pair"),
            c,
            1.0,
        ),
    ]
}

/// A straight-sided spline of `teeth` teeth, 28 over 23 mm, with its hub.
fn small_pair(teeth: u32) -> Pair {
    let shaft = StraightSpline {
        teeth,
        major_diameter: 28.0,
        minor_diameter: 23.0,
        tooth_width: 4.0,
        root_round: 0.3,
        tip: TipCorner::Chamfer { size: 0.3 },
    };
    let fit = straight::Fit {
        side_clearance: 0.05,
        major_clearance: 0.3,
        minor_clearance: 0.2,
        hub_root_round: 0.2,
        hub_tip_round: 0.3,
    };
    straight::spline_pair(&shaft, &fit, 1e-3).expect("a pair")
}

fn least(pair: &Pair, reach: f64) -> f64 {
    pair.through_a_cycle(6, reach)
        .ok()
        .and_then(|g| g.least)
        .map_or(f64::NAN, |g| g.distance)
}

/// An angle brought to -pi..pi.
fn wrapped(angle: f64) -> f64 {
    (angle + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0
}

/// The clearance is kept and the hub goes on in one position only: checked on a pair with its
/// master spline.
fn one_way_only(name: &str, plain: &Pair, keyed: &Pair, tolerance: f64, reach: f64) {
    check_outline(name, &keyed.first.outline);
    check_outline(name, &keyed.second.outline);
    assert_eq!(
        (keyed.first.outline.teeth(), keyed.second.outline.teeth()),
        (1, 1),
        "{name}"
    );
    assert_eq!(
        (keyed.second_start, keyed.cycle),
        (plain.second_start, TAU),
        "{name}"
    );
    // The clearance is not reduced: the least gap through a turn is the plain pair's, to the
    // tolerance.
    let (without, with_key) = (least(plain, reach), least(keyed, reach));
    assert!(
        with_key >= without - tolerance,
        "{name}: least gap {with_key} with it, {without} without"
    );
    // Turned on by one pitch, the hub meets the shaft: it goes on in one position only.
    let pitch = plain.first.outline.pitch_angle();
    let mut turned = keyed.clone();
    turned.second_start += pitch;
    let overlap = turned.gap_at(0.0, reach).ok().flatten().map(|g| g.distance);
    assert!(
        overlap.is_some_and(|d| d < 0.0),
        "{name}: turned on by one pitch the gap is {overlap:?}"
    );
}

#[test]
fn a_master_spline_takes_no_clearance_away_and_lets_the_hub_go_on_one_way_only() {
    for (name, pair, tolerance, reach) in default_pairs() {
        let teeth = pair.first.outline.teeth();
        let pitch = TAU / f64::from(teeth);
        // Without the master spline a turn of one pitch fits.
        let mut plain_turned = pair.clone();
        plain_turned.second_start += pitch;
        let fits = plain_turned
            .gap_at(0.0, reach)
            .ok()
            .flatten()
            .map(|g| g.distance);
        assert!(fits.is_some_and(|d| d > 0.0), "{name}: {fits:?}");
        for form in [Form::ToothLeftOut, Form::WideTooth] {
            let (keyed, _, _) = master_pair(&pair, form, teeth / 2)
                .unwrap_or_else(|e| panic!("{name} {form:?}: {e}"));
            one_way_only(&format!("{name} {form:?}"), &pair, &keyed, tolerance, reach);
            // The same once the whole spline is turned to put the master spline at an angle.
            for angle in [-360.0, 360.0, -135.0, 271.3] {
                let m = master_spline(&pair, form, angle)
                    .unwrap_or_else(|e| panic!("{name} {form:?} {angle}: {e}"));
                one_way_only(
                    &format!("{name} {form:?} at {angle}"),
                    &pair,
                    &m.pair,
                    tolerance,
                    reach,
                );
            }
        }
    }
}

#[test]
fn the_period_left_out_is_one_arc_on_the_circle_and_every_other_piece_is_kept() {
    for (name, pair, _, _) in default_pairs() {
        let shaft = &pair.first.outline;
        let (low, high) = shaft.radii();
        for (circle, radius, role) in [
            (Circle::Smallest, low, Role::Root),
            (Circle::Largest, high, Role::Tip),
        ] {
            let replaced = replaced_period(shaft, Material::Inside, circle, 0.0)
                .unwrap_or_else(|e| panic!("{name} {circle:?}: {e}"));
            let pieces = replaced.outline.tooth();
            let pitch = shaft.pitch_angle();
            // The sweep of an arc about the axis on the circle with the circle's role.
            let on_circle = |p: &Piece| match p {
                Piece::Arc {
                    centre,
                    radius: r,
                    sweep,
                    role: found,
                    ..
                } if centre == &[0.0, 0.0] && (r - radius).abs() < 1e-9 && *found == role => {
                    Some(*sweep)
                }
                _ => None,
            };
            let own: Vec<f64> = shaft.tooth().iter().filter_map(on_circle).collect();
            let now: Vec<f64> = pieces.iter().filter_map(on_circle).collect();
            // One period gone, one arc in its place, and that arc one piece with the outline's
            // own arcs on the circle that it runs on into: the one before, the one after, or both.
            let whole = shaft.tooth().len() * (shaft.teeth() as usize - 1) + 1;
            let joined = whole - pieces.len();
            assert!(joined <= 2, "{name} {circle:?}: {joined} pieces joined");
            // A period runs from a joint on the circle, which is an end of the outline's own arc
            // there where it has one: the new arc then has that arc beside it, and joins it.
            assert_eq!(joined == 0, own.is_empty(), "{name} {circle:?}");
            assert_eq!(
                now.len(),
                own.len() * (shaft.teeth() as usize - 1) + 1 - joined,
                "{name} {circle:?}"
            );
            // Going round, the arcs on the circle cover what the outline's own covered on every
            // other period, and a pitch more: nothing is added or lost by the joining.
            let covered: f64 = now.iter().sum();
            let before: f64 = own.iter().sum::<f64>() * f64::from(shaft.teeth() - 1);
            assert!(
                (covered - before - pitch).abs() < 1e-9,
                "{name} {circle:?}: {covered} against {before} and a pitch"
            );
            // The arc of the period is the one arc at least a pitch long, and no arc on the
            // circle is left beside it: the stretch is one piece from side to side.
            let long: Vec<usize> = pieces
                .iter()
                .enumerate()
                .filter(|(_, p)| on_circle(p).is_some_and(|s| s > pitch - 1e-12))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(long.len(), 1, "{name} {circle:?}");
            let (at, n) = (long[0], pieces.len());
            let sweep = on_circle(&pieces[at]).unwrap();
            assert_eq!(
                joined == 0,
                (sweep - pitch).abs() < 1e-12,
                "{name} {circle:?}"
            );
            for side in [(at + n - 1) % n, (at + 1) % n] {
                assert!(
                    on_circle(&pieces[side]).is_none(),
                    "{name} {circle:?}: an arc on the circle left beside the period's arc"
                );
            }
            assert!(
                (replaced.arc.radius - radius).abs() < 1e-9
                    && (replaced.arc.end - replaced.arc.start - shaft.pitch_angle()).abs() < 1e-12
            );
            // The angle 0 lies within the arc: the tooth on the x axis is the one left out, or the
            // space after it.
            let within = (0.0 - replaced.arc.start).rem_euclid(TAU) <= shaft.pitch_angle();
            assert!(within, "{name} {circle:?}: {:?}", replaced.arc);
        }
    }
}

/// The placement asked for, field by field: the tooth, the middle (exact, a whole count or a half)
/// and the turn in degrees (to a rounding).
fn assert_placement(teeth: u32, form: Form, angle: f64, (tooth, middle, turn): (u32, f64, f64)) {
    let p: Placement =
        placement(teeth, form, angle).unwrap_or_else(|e| panic!("{teeth} {form:?} {angle}: {e}"));
    assert_eq!(
        (p.tooth, p.middle),
        (tooth, middle),
        "{teeth} {form:?} at {angle}: {p:?}"
    );
    assert!(
        (p.turn.to_degrees() - turn).abs() < 1e-9,
        "{teeth} {form:?} at {angle}: {p:?}, not a turn of {turn}"
    );
}

#[test]
fn the_master_spline_sits_exactly_at_its_angle_and_the_whole_spline_turns_to_put_it_there() {
    // A tooth left out where a tooth stands needs no turn; at 100 degrees on 20 teeth the nearest
    // is tooth 6 (108 degrees), so the whole spline turns by -8 degrees.
    assert_placement(20, Form::ToothLeftOut, 0.0, (0, 0.0, 0.0));
    assert_placement(20, Form::ToothLeftOut, 100.0, (6, 6.0, -8.0));
    // The middle of a wide tooth is half a pitch on from a tooth: on 20 teeth the spline turns back
    // by 9 degrees at the bottom as at the top.
    assert_placement(20, Form::WideTooth, 0.0, (0, 0.5, -9.0));
    assert_placement(20, Form::WideTooth, 180.0, (10, 10.5, -9.0));
    // A negative angle counts the other way round; an angle just between two teeth takes the one
    // further on.
    assert_placement(4, Form::ToothLeftOut, -90.0, (3, -1.0, 0.0));
    assert_placement(4, Form::ToothLeftOut, 45.0, (1, 1.0, -45.0));
    // A whole turn either way is no turn at all for a tooth left out, and half a pitch back for a
    // wide tooth (just between two wide teeth, the one further on).
    for teeth in [3, 4, 5, 7, 20, 36] {
        let z = f64::from(teeth);
        let half = 180.0 / z;
        assert_placement(teeth, Form::ToothLeftOut, -360.0, (0, -z, 0.0));
        assert_placement(teeth, Form::ToothLeftOut, 360.0, (0, z, 0.0));
        assert_placement(teeth, Form::WideTooth, -360.0, (0, -z + 0.5, -half));
        assert_placement(teeth, Form::WideTooth, 360.0, (0, z + 0.5, -half));
    }
    // Odd counts: half a turn falls just between two teeth (the one further on is taken, half a
    // pitch back) and exactly on the middle of a space (a wide tooth needs no turn).
    for teeth in [3u32, 5, 7] {
        let (z, half) = (f64::from(teeth), 180.0 / f64::from(teeth));
        assert_placement(
            teeth,
            Form::ToothLeftOut,
            -180.0,
            (teeth.div_ceil(2), -(z - 1.0) / 2.0, -half),
        );
        assert_placement(
            teeth,
            Form::WideTooth,
            -180.0,
            ((teeth - 1) / 2, -z / 2.0, 0.0),
        );
        assert_placement(
            teeth,
            Form::ToothLeftOut,
            180.0,
            (teeth.div_ceil(2), (z + 1.0) / 2.0, -half),
        );
        assert_placement(
            teeth,
            Form::WideTooth,
            180.0,
            ((teeth - 1) / 2, z / 2.0, 0.0),
        );
        // Halfway between tooth n and tooth n + 1: the tooth n + 1 is left out, or the wide tooth
        // after n needs no turn.
        for n in [1i32, -2] {
            let angle = (f64::from(n) + 0.5) * 2.0 * half;
            let after = u32::try_from((n + 1).rem_euclid(teeth as i32)).expect("a tooth");
            let at = u32::try_from(n.rem_euclid(teeth as i32)).expect("a tooth");
            assert_placement(
                teeth,
                Form::ToothLeftOut,
                angle,
                (after, f64::from(n + 1), -half),
            );
            assert_placement(teeth, Form::WideTooth, angle, (at, f64::from(n) + 0.5, 0.0));
        }
    }
    // On the parts: the master spline's middle is the angle given. The arcs that replace a period,
    // on the shaft and (with the hub's start added) in the hub, run from joint to joint, so each is
    // off that middle by as much as its joints are off the tooth's or the space's middle: less than
    // a quarter of a pitch on these three kinds (the most is about 0.16 of a pitch, the involute
    // hub's), so each covers the middle. That offset is the same at every angle, the hub's as the
    // shaft's: both are turned together, by the same turn.
    for (name, pair, _, _) in default_pairs() {
        let pitch = pair.first.outline.pitch_angle();
        for form in [Form::ToothLeftOut, Form::WideTooth] {
            let mut first: Option<(f64, f64)> = None;
            for angle in [0.0, 100.0, -135.0, 271.3, -360.0, 360.0] {
                let m = master_spline(&pair, form, angle)
                    .unwrap_or_else(|e| panic!("{name} {form:?}: {e}"));
                assert!(
                    wrapped(m.middle - f64::to_radians(angle)).abs() < 1e-12,
                    "{name} {form:?} {angle}: {m:?}"
                );
                let shaft_off = wrapped((m.shaft_arc.start + m.shaft_arc.end) / 2.0 - m.middle);
                let hub_off = wrapped(
                    (m.hub_arc.start + m.hub_arc.end) / 2.0 + m.pair.second_start - m.middle,
                );
                for (part, off) in [("shaft", shaft_off), ("hub", hub_off)] {
                    assert!(
                        off.abs() < 0.25 * pitch,
                        "{name} {form:?} {angle}: the {part}'s arc is {off} off"
                    );
                }
                let (shaft_was, hub_was) = *first.get_or_insert((shaft_off, hub_off));
                assert!(
                    (shaft_off - shaft_was).abs() < 1e-9 && (hub_off - hub_was).abs() < 1e-9,
                    "{name} {form:?} {angle}: the arcs are off by {shaft_off} and {hub_off}, at the first angle by \
                     {shaft_was} and {hub_was}"
                );
                assert!(
                    (m.pair.first.outline.radii().0 - pair.first.outline.radii().0).abs() < 1e-9
                );
            }
        }
    }
}

/// Whether a point lies inside a closed chain of points (crossings of a ray to the right, counted).
fn inside(points: &[Point], q: Point) -> bool {
    let mut inside = false;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let x = a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x > q[0] {
                inside = !inside;
            }
        }
    }
    inside
}

/// The turn of the whole spline that puts the master spline's middle at `angle` degrees, worked out
/// on its own: a tooth's middle is a whole count of pitches from the x axis, a space's a half more;
/// the nearest is taken, and just between two, the one further on.
fn expected_turn(teeth: u32, form: Form, angle: f64) -> f64 {
    let pitch = 360.0 / f64::from(teeth);
    let half = if form == Form::WideTooth { 0.5 } else { 0.0 };
    let nearest = (angle / pitch - half + 0.5).floor() + half;
    (angle - nearest * pitch).to_radians()
}

/// Around the whole circle at `radius`, whether each of `samples` points is inside `outline`, from
/// `angle` on.
fn material_around(outline: &Outline, radius: f64, angle: f64, samples: u32) -> Vec<(f64, bool)> {
    let points = outline.points();
    (0..samples)
        .map(|i| {
            // Off the round fractions of a pitch, so that no point falls on a corner of the
            // outline.
            let off = (f64::from(i) + 0.37) / f64::from(samples) * TAU;
            let a = angle + off;
            (
                wrapped(off),
                inside(&points, [radius * det::cos(a), radius * det::sin(a)]),
            )
        })
        .collect()
}

#[test]
fn the_tooth_left_out_or_the_space_filled_is_the_one_at_the_angle_and_the_rest_is_the_spline_turned(
) {
    // Around the circle halfway between the shaft's root and tip circles: within half a pitch of
    // the angle there is no material where a tooth is left out and material all across where a
    // space is filled; everywhere else the shaft is the ordinary shaft turned by the turn worked
    // out here. The hub's opening the same way round: within half a pitch of the angle its wide
    // tooth fills the opening (tooth left out) or its tooth is gone (wide tooth), and elsewhere it
    // is the ordinary hub turned with the shaft.
    for (name, pair, _, _) in default_pairs() {
        let teeth = pair.first.outline.teeth();
        let pitch = pair.first.outline.pitch_angle();
        let samples = 32 * teeth;
        let mean = |o: &Outline| (o.radii().0 + o.radii().1) / 2.0;
        for angle in [
            0.0, 60.0, 100.0, -170.0, 333.0, -360.0, 360.0, -135.0, 271.3,
        ] {
            for form in [Form::ToothLeftOut, Form::WideTooth] {
                let m = master_spline(&pair, form, angle)
                    .unwrap_or_else(|e| panic!("{name} {form:?}: {e}"));
                let turn = expected_turn(teeth, form, angle);
                assert!(
                    (m.placement.turn - turn).abs() < 1e-12,
                    "{name} {form:?} {angle}: {m:?}"
                );
                let at = f64::to_radians(angle);
                for (part, keyed, plain, start) in [
                    ("shaft", &m.pair.first.outline, &pair.first.outline, 0.0),
                    (
                        "hub",
                        &m.pair.second.outline,
                        &pair.second.outline,
                        pair.second_start,
                    ),
                ] {
                    let radius = mean(plain);
                    let ordinary = master_spline::turned(plain, turn).expect("turned");
                    let found = material_around(keyed, radius, at - start, samples);
                    let wanted = material_around(&ordinary, radius, at - start, samples);
                    // Inside the shaft's outline is material, inside the hub's opening is none: at
                    // the master spline both are inside for a wide tooth (the shaft's space filled,
                    // the hub's tooth gone) and outside for a tooth left out.
                    let master = form == Form::WideTooth;
                    for ((off, here), (_, ordinary_here)) in found.iter().zip(&wanted) {
                        let expected = if off.abs() < pitch / 2.0 {
                            master
                        } else {
                            *ordinary_here
                        };
                        assert_eq!(
                            *here,
                            expected,
                            "{name} {form:?} at {angle}: the {part} {} pitches on",
                            off / pitch
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn refusals_and_hostile_numbers() {
    let pairs = default_pairs();
    let (_, pair, _, _) = &pairs[0];
    assert_eq!(
        out_of_range(&placement(1, Form::ToothLeftOut, 0.0)),
        Some("teeth")
    );
    assert_eq!(
        out_of_range(&placement(0, Form::WideTooth, 0.0)),
        Some("teeth")
    );
    assert_eq!(
        out_of_range(&placement(u32::MAX, Form::WideTooth, 0.0)),
        Some("teeth")
    );
    for angle in [
        361.0,
        -361.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e300,
    ] {
        assert_eq!(
            why(&placement(20, Form::ToothLeftOut, angle)),
            Some(Why::MasterAngleRange),
            "{angle}"
        );
    }
    assert_eq!(
        out_of_range(&master_pair(pair, Form::ToothLeftOut, 20)),
        Some("tooth")
    );
    assert_eq!(
        out_of_range(&master_pair(pair, Form::ToothLeftOut, u32::MAX)),
        Some("tooth")
    );
    // An outline of one tooth has no period to spare.
    let once = replaced_period(&pair.first.outline, Material::Inside, Circle::Smallest, 0.0)
        .expect("replaced")
        .outline;
    assert_eq!(
        out_of_range(&replaced_period(
            &once,
            Material::Inside,
            Circle::Smallest,
            0.0
        )),
        Some("teeth")
    );
    // An angle that is not a number is refused in plain words.
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            why(&master_spline::turned(&pair.first.outline, v)),
            Some(Why::AngleNotANumber),
            "{v}"
        );
        let replaced = replaced_period(&pair.first.outline, Material::Inside, Circle::Largest, v);
        assert_eq!(why(&replaced), Some(Why::AngleNotANumber), "{v}");
    }
    assert_eq!(
        TeethFailure::Invalid(Why::AngleNotANumber).to_string(),
        "the angle is not a number"
    );
    for v in HOSTILE {
        let _ = placement(20, Form::WideTooth, v);
        let _ = master_spline(pair, Form::ToothLeftOut, v);
        let r = replaced_period(&pair.first.outline, Material::Outside, Circle::Largest, v);
        assert_eq!(r.is_ok(), v.is_finite(), "{v}");
        let mut odd = pair.clone();
        odd.second_start = v;
        let r = master_pair(&odd, Form::WideTooth, 3);
        if let Ok((keyed, _, _)) = r {
            assert!(keyed
                .first
                .outline
                .points()
                .iter()
                .flatten()
                .all(|x| x.is_finite()));
        }
    }
}

#[test]
fn a_pair_that_is_not_a_shaft_and_its_hub_is_refused() {
    let pair = small_pair(6);
    let refused = Some(Why::NotAShaftAndHub);
    let mut other_count = pair.clone();
    other_count.second = small_pair(5).second;
    let mut opposite = pair.clone();
    opposite.motion = Motion::Turning { ratio: -1.0 };
    let mut twice = pair.clone();
    twice.motion = Motion::Turning { ratio: 2.0 };
    let mut apart = pair.clone();
    apart.second.axis = [40.0, 0.0];
    let mut two_shafts = pair.clone();
    two_shafts.second.material = Material::Inside;
    let mut inside_out = pair.clone();
    inside_out.first.material = Material::Outside;
    for (what, odd) in [
        ("other count", other_count),
        ("opposite", opposite),
        ("twice", twice),
        ("apart", apart),
        ("two shafts", two_shafts),
        ("inside out", inside_out),
    ] {
        for form in [Form::ToothLeftOut, Form::WideTooth] {
            assert_eq!(why(&master_pair(&odd, form, 0)), refused, "{what} {form:?}");
            assert_eq!(
                why(&master_spline(&odd, form, 0.0)),
                refused,
                "{what} {form:?}"
            );
        }
    }
    assert!(master_pair(&pair, Form::WideTooth, 0).is_ok());
}

/// A tooth of `teeth` teeth made of curves only, from radius 10 at the x axis to `middle` half a
/// pitch on and back to 10 a pitch on: as one curve, or as two that join at `middle`.
fn curved(teeth: u32, middle: f64, two: bool) -> Outline {
    let pitch = TAU / f64::from(teeth);
    let at = |share: f64| {
        let r = 10.0 + (middle - 10.0) * (1.0 - (2.0 * share - 1.0).abs());
        [r * det::cos(share * pitch), r * det::sin(share * pitch)]
    };
    let up: Vec<Point> = (0..=8).map(|i| at(f64::from(i) / 16.0)).collect();
    let down: Vec<Point> = (8..=16).map(|i| at(f64::from(i) / 16.0)).collect();
    let tooth = if two {
        vec![
            Piece::Curve {
                points: up,
                role: Role::Flank,
            },
            Piece::Curve {
                points: down,
                role: Role::Flank,
            },
        ]
    } else {
        let all = (0..=16).map(|i| at(f64::from(i) / 16.0)).collect();
        vec![Piece::Curve {
            points: all,
            role: Role::Flank,
        }]
    };
    Outline::new(teeth, tooth, 1e-3).expect("a curved outline")
}

#[test]
fn a_period_needs_a_corner_on_its_circle_and_its_arc_takes_the_role_that_circle_has() {
    // Pointed tips with corners only on the root circle: nothing to join on the largest circle; and
    // pointed spaces with corners only on the tip circle: nothing to join on the smallest.
    let tips = curved(6, 11.0, false);
    let spaces = curved(6, 9.0, false);
    assert_eq!(
        why(&replaced_period(
            &tips,
            Material::Inside,
            Circle::Largest,
            0.0
        )),
        Some(Why::NoCornerOnLargest)
    );
    assert_eq!(
        why(&replaced_period(
            &spaces,
            Material::Inside,
            Circle::Smallest,
            0.0
        )),
        Some(Why::NoCornerOnSmallest)
    );
    assert!(replaced_period(&tips, Material::Inside, Circle::Smallest, 0.0).is_ok());
    assert!(replaced_period(&spaces, Material::Inside, Circle::Largest, 0.0).is_ok());
    // On a pair, the shaft's refusal comes through master_pair and master_spline as it is: the
    // same reason, named by the function that was called.
    let mut pair = small_pair(6);
    pair.first.outline = tips;
    let keyed = master_pair(&pair, Form::WideTooth, 0);
    assert_eq!(why(&keyed), Some(Why::NoCornerOnLargest));
    assert_eq!(operation(&keyed), Some("teeth::master_spline::master_pair"));
    let turned = master_spline(&pair, Form::WideTooth, 10.0);
    assert_eq!(why(&turned), Some(Why::NoCornerOnLargest));
    assert_eq!(
        operation(&turned),
        Some("teeth::master_spline::master_spline")
    );
    pair.first.outline = spaces;
    assert_eq!(
        why(&master_pair(&pair, Form::ToothLeftOut, 0)),
        Some(Why::NoCornerOnSmallest)
    );
    // With corners on both circles but no arc about the axis on either, the arc takes the circle's
    // role on the part: root and tip on a shaft, tip and root in a hub's opening.
    let zigzag = curved(6, 11.0, true);
    for (material, circle, role) in [
        (Material::Inside, Circle::Smallest, Role::Root),
        (Material::Inside, Circle::Largest, Role::Tip),
        (Material::Outside, Circle::Smallest, Role::Tip),
        (Material::Outside, Circle::Largest, Role::Root),
    ] {
        let replaced = replaced_period(&zigzag, material, circle, 0.0).expect("replaced");
        let arc = replaced
            .outline
            .tooth()
            .iter()
            .find(|p| matches!(p, Piece::Arc { .. }))
            .expect("the arc");
        assert_eq!(arc.role(), role, "{material:?} {circle:?}");
    }
}

#[test]
fn a_tip_rounded_all_across_still_has_a_corner_on_the_major_circle_and_takes_a_wide_tooth() {
    // Rounds as large as half the tooth is wide meet on the major circle with no flat between them:
    // the corner where they meet lies on that circle, so a period can be closed there.
    let shaft = StraightSpline {
        teeth: 6,
        major_diameter: 28.0,
        minor_diameter: 23.0,
        tooth_width: 2.0,
        root_round: 0.0,
        tip: TipCorner::Round { radius: 1.0 },
    };
    let fit = straight::Fit {
        side_clearance: 0.05,
        major_clearance: 0.3,
        minor_clearance: 0.2,
        hub_root_round: 0.0,
        hub_tip_round: 0.0,
    };
    let pair =
        straight::spline_pair(&shaft, &fit, 1e-3).expect("a pair with tips rounded all across");
    // No arc about the axis on the major circle: the rounds are centred off it.
    assert!(
        !pair
            .first
            .outline
            .tooth()
            .iter()
            .any(|p| matches!(p, Piece::Arc { centre, .. } if det::hypot(centre[0], centre[1]) < 1e-9 && p.role() == Role::Tip))
    );
    let (keyed, shaft_arc, _) = master_pair(&pair, Form::WideTooth, 0).expect("a wide tooth");
    check_outline("rounded", &keyed.first.outline);
    check_outline("rounded", &keyed.second.outline);
    assert!((shaft_arc.radius - 14.0).abs() < 1e-9, "{shaft_arc:?}");
    let arc = keyed
        .first
        .outline
        .tooth()
        .iter()
        .find(|p| matches!(p, Piece::Arc { centre, .. } if centre == &[0.0, 0.0]))
        .expect("the arc of the wide tooth");
    assert_eq!(arc.role(), Role::Tip);
    assert!(least(&keyed, 3.0) > 0.0);
    // A tooth left out needs a corner on the root circle, which every shaft of the engine has.
    assert!(master_pair(&pair, Form::ToothLeftOut, 0).is_ok());
}

#[test]
fn the_smallest_counts_of_teeth_take_a_master_spline() {
    for teeth in [2, 3, 4] {
        let pair = small_pair(teeth);
        for form in [Form::ToothLeftOut, Form::WideTooth] {
            let m = master_spline(&pair, form, 90.0)
                .unwrap_or_else(|e| panic!("{teeth} {form:?}: {e}"));
            check_outline("small", &m.pair.first.outline);
            check_outline("small", &m.pair.second.outline);
            assert!(least(&m.pair, 3.0) > 0.0, "{teeth} {form:?}");
        }
    }
}
