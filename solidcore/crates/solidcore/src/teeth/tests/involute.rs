//! Involute splines, the shaft's teeth and the hub's tooth spaces, checked against their definition
//! rather than against their formulas: a shaft's tooth must be exactly what the rack-shaped tool
//! leaves. No outline that is accepted crosses itself, and a hub whose root rounds would reach the
//! shaft's tips is refused exactly there: each shaft and hub the functions accept is turned and
//! must run clear; each they refuse for an overlap is put together by hand, turned, and must
//! overlap.

use crate::math::det;
use std::f64::consts::{PI, TAU};

use super::basics::old_sampled;
use super::common::{failure, lift, not_proved, operation, typed_why, why, Dice};
use crate::blend::fillet_section::interval::Ivl;
use crate::error::teeth::{TeethFailure, Why, FILLET_SEPARATION};
use crate::teeth::bound::Bend;
use crate::teeth::involute::{hub_for, inv, spline_pair, External, Fillet, Fit, Internal, Wedge};
use crate::teeth::outline::{rotated, sampled};
use crate::teeth::{Gaps, Material, Outline, Pair, Part, Piece, Point, Role};

fn shaft_of(module: f64, teeth: u32, shift: f64) -> External {
    External {
        module,
        teeth,
        pressure_angle: 20.0,
        addendum: 1.0,
        dedendum: 1.25,
        shift,
        tool_tip_radius: 0.3,
        thinning: 0.0,
    }
}

fn spline_shaft() -> External {
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

/// A spread of teeth: plain, shifted, few (undercut), steep flanks, a sharp tool, thinned.
fn cases() -> Vec<(&'static str, External)> {
    vec![
        ("shaft 20", shaft_of(2.0, 20, 0.0)),
        ("shaft 7, undercut", shaft_of(3.0, 7, 0.0)),
        ("shaft 9, undercut", shaft_of(1.0, 9, -0.1)),
        ("shaft 10 shifted", shaft_of(3.0, 10, 0.5)),
        ("shaft 120", shaft_of(0.5, 120, 0.2)),
        ("spline shaft", spline_shaft()),
        (
            "sharp tool",
            External {
                tool_tip_radius: 0.0,
                ..shaft_of(2.0, 30, 0.0)
            },
        ),
        (
            "sharp tool, undercut",
            External {
                tool_tip_radius: 0.0,
                ..shaft_of(2.0, 8, 0.0)
            },
        ),
        (
            "thinned",
            External {
                thinning: 0.12,
                ..shaft_of(2.5, 17, 0.1)
            },
        ),
        (
            "full round",
            External {
                tool_tip_radius: 0.47,
                ..shaft_of(2.0, 24, 0.0)
            },
        ),
    ]
}

fn dist(a: Point, b: Point) -> f64 {
    det::hypot(a[0] - b[0], a[1] - b[1])
}

#[test]
fn the_circles_of_a_plain_shaft() -> crate::Result<()> {
    let g = shaft_of(2.0, 20, 0.0);
    assert!((g.pitch_radius()? - 20.0).abs() < 1e-12);
    assert!((g.base_radius()? - 20.0 * det::cos(20f64.to_radians())).abs() < 1e-12);
    assert!((g.tip_radius()? - 22.0).abs() < 1e-12);
    assert!((g.root_radius()? - 17.5).abs() < 1e-12);
    assert!((g.tooth_thickness()? - PI).abs() < 1e-12);
    // A shift moves tip and root and thickens the tooth.
    let shifted = shaft_of(2.0, 20, 0.5);
    assert!((shifted.tip_radius()? - 23.0).abs() < 1e-12);
    assert!((shifted.root_radius()? - 18.5).abs() < 1e-12);
    assert!((shifted.tooth_thickness()? - (PI + 2.0 * det::tan(20f64.to_radians()))).abs() < 1e-12);
    Ok(())
}

#[test]
fn the_involute_function() {
    // tan(a) - a at 20 degrees, worked out by hand: 0.363970 - 0.349066. And nothing at 0.
    assert!((inv(20f64.to_radians()) - 0.014904).abs() < 1e-6);
    assert_eq!(inv(0.0), 0.0);
    // It rises with the angle.
    let values: Vec<f64> = [0.5, 5.0, 14.5, 20.0, 30.0, 45.0, 60.0, 80.0]
        .map(|d: f64| inv(d.to_radians()))
        .to_vec();
    assert!(values.windows(2).all(|w| w[0] < w[1]), "{values:?}");
}

/// The pieces of one tooth and the outline's own promise: they join, and the tooth closes onto the
/// next.
fn check_chain(name: &str, outline: &Outline) {
    let tooth = outline.tooth();
    for pair in tooth.windows(2) {
        let (end, start) = (pair[0].end().unwrap(), pair[1].start().unwrap());
        assert!(
            dist(end, start) < 1e-9,
            "{name}: pieces {:?} and {:?} do not join",
            pair[0].role(),
            pair[1].role()
        );
    }
    let (first, last) = (
        tooth.first().unwrap().start().unwrap(),
        tooth.last().unwrap().end().unwrap(),
    );
    assert!(
        dist(last, rotated(first, outline.pitch_angle())) < 1e-9,
        "{name}: the tooth does not close"
    );
}

#[test]
fn every_outline_is_a_closed_chain_between_root_and_tip() -> crate::Result<()> {
    for (name, e) in cases() {
        let outline = e.outline(1e-6)?;
        check_chain(name, &outline);
        let roles: Vec<Role> = outline.tooth().iter().map(Piece::role).collect();
        assert_eq!(
            roles.get(..5),
            Some(
                &[
                    Role::Fillet,
                    Role::Flank,
                    Role::Tip,
                    Role::Flank,
                    Role::Fillet
                ][..]
            ),
            "{name}"
        );
        let (low, high) = outline.radii();
        assert!(
            (low - e.root_radius()?).abs() < 1e-9,
            "{name}: lowest {low}"
        );
        assert!(
            (high - e.tip_radius()?).abs() < 1e-9,
            "{name}: highest {high}"
        );
        // Counter-clockwise, and as many teeth as asked for.
        let points = outline.points();
        let doubled: f64 = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum();
        assert!(doubled > 0.0, "{name}");
        assert_eq!(outline.teeth(), e.teeth);
    }
    Ok(())
}

#[test]
fn the_flank_is_the_involute_of_the_base_circle() -> crate::Result<()> {
    for (name, e) in cases() {
        let outline = e.outline(1e-3)?;
        let (r, rb) = (e.pitch_radius()?, e.base_radius()?);
        let half_at_pitch = e.tooth_thickness()? / (2.0 * r);
        for piece in outline.tooth() {
            let Piece::Curve {
                points,
                role: Role::Flank,
            } = piece
            else {
                continue;
            };
            for &[x, y] in points {
                let radius = det::hypot(x, y);
                assert!(
                    radius >= rb * (1.0 - 1e-12),
                    "{name}: a flank point below the base circle"
                );
                let at_radius = det::acos((rb / radius).min(1.0));
                let expected = half_at_pitch + inv(e.pressure_angle.to_radians()) - inv(at_radius);
                assert!(
                    (det::atan2(y, x).abs() - expected).abs() < 1e-11,
                    "{name}: off the involute at radius {radius}"
                );
                assert!(
                    (e.half_angle_at(radius)? - expected).abs() < 1e-11,
                    "{name}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn the_fillet_runs_into_the_flank_smoothly_unless_the_tooth_is_undercut() -> crate::Result<()> {
    for (name, e) in cases() {
        let outline = e.outline(1e-8)?;
        let [Piece::Curve { points: fillet, .. }, Piece::Curve { points: flank, .. }, ..] =
            outline.tooth()
        else {
            panic!("{name}: the tooth does not start with fillet and flank");
        };
        let n = fillet.len();
        let into = [
            fillet[n - 1][0] - fillet[n - 2][0],
            fillet[n - 1][1] - fillet[n - 2][1],
        ];
        let out_of = [flank[1][0] - flank[0][0], flank[1][1] - flank[0][1]];
        let turn = det::atan2(
            into[0] * out_of[1] - into[1] * out_of[0],
            into[0] * out_of[0] + into[1] * out_of[1],
        )
        .abs();
        if e.is_undercut()? {
            assert!(
                turn > 0.05,
                "{name}: an undercut tooth has a corner where the fillet cuts the flank ({turn})"
            );
            assert!(e.form_radius()? > e.base_radius()?, "{name}");
        } else {
            assert!(
                turn < 0.02,
                "{name}: the fillet should run into the flank without a corner ({turn})"
            );
        }
    }
    assert!(shaft_of(3.0, 7, 0.0).is_undercut()?);
    assert!(!shaft_of(2.0, 20, 0.0).is_undercut()? && !shaft_of(3.0, 10, 0.5).is_undercut()?);
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// The definition: a rack rolling on the pitch circle
// -------------------------------------------------------------------------------------------------

/// The rack that cuts `e`, written down from its description alone, as the test's own model of the
/// tool.
struct Rack {
    r: f64,
    pitch: f64,
    sin: f64,
    cos: f64,
    rho: f64,
    /// The corners of a rack tooth drawn in by the tip round's radius: the centres of its two
    /// rounds.
    left: Point,
    right: Point,
}

impl Rack {
    fn of(e: &External) -> Rack {
        let (sin, cos) = det::sin_cos(e.pressure_angle.to_radians());
        let (m, pitch) = (e.module, PI * e.module);
        let thickness = pitch / 2.0 + 2.0 * e.shift * m * sin / cos - e.thinning;
        let (rho, depth) = (e.tool_tip_radius * m, (e.dedendum - e.shift) * m);
        // A rack tooth between two spaces: its flanks pass through the line of rolling at half the
        // shaft tooth's thickness from the middle of each space, and lean so that the tooth narrows
        // towards its tip.
        let across = rho - depth; // height of the rounds' centres
        let along = thickness / 2.0 + (rho - across * sin) / cos;
        Rack {
            r: m * f64::from(e.teeth) / 2.0,
            pitch,
            sin,
            cos,
            rho,
            left: [along, across],
            right: [pitch - along, across],
        }
    }

    /// The distance from a point of the rack's plane to the surface of one rack tooth, negative
    /// inside it.
    fn one_tooth(&self, u: f64, v: f64) -> f64 {
        // The tooth drawn in by the round's radius is bounded by two rays and the piece between the
        // centres.
        let ray = |c: Point, dir: Point| {
            let t = ((u - c[0]) * dir[0] + (v - c[1]) * dir[1]).max(0.0);
            det::hypot(u - c[0] - t * dir[0], v - c[1] - t * dir[1])
        };
        let between = det::hypot(
            u - u.clamp(self.left[0], self.right[0].max(self.left[0])),
            v - self.left[1],
        );
        let to_drawn_in = ray(self.left, [-self.sin, self.cos])
            .min(ray(self.right, [self.sin, self.cos]))
            .min(between);
        let inside = (u - self.left[0]) * self.cos + (v - self.left[1]) * self.sin >= 0.0
            && (self.right[0] - u) * self.cos + (v - self.right[1]) * self.sin >= 0.0
            && v >= self.left[1];
        if inside {
            -to_drawn_in - self.rho
        } else {
            to_drawn_in - self.rho
        }
    }

    /// The distance from a point of the shaft to the rack when the rack has rolled to the angle
    /// `phi`.
    fn distance(&self, g: Point, phi: f64) -> f64 {
        let (sn, cs) = det::sin_cos(phi);
        let u = self.r * phi + (-g[0] * sn + g[1] * cs);
        let v = g[0] * cs + g[1] * sn - self.r;
        let u = u.rem_euclid(self.pitch);
        self.one_tooth(u, v)
            .min(self.one_tooth(u - self.pitch, v))
            .min(self.one_tooth(u + self.pitch, v))
    }

    /// The closest the rack ever comes to a point of the shaft while it rolls past.
    fn closest(&self, g: Point, window: f64) -> f64 {
        const STEPS: usize = 20_000;
        let middle = det::atan2(g[1], g[0]);
        let at = |i: usize| middle - window + 2.0 * window * (i as f64) / (STEPS as f64);
        let (mut best, mut best_i) = (f64::INFINITY, 0);
        for i in 0..=STEPS {
            let d = self.distance(g, at(i));
            if d < best {
                (best, best_i) = (d, i);
            }
        }
        // Finer around the best step.
        let (mut lo, mut hi) = (at(best_i.saturating_sub(1)), at((best_i + 1).min(STEPS)));
        for _ in 0..200 {
            let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if self.distance(g, a) < self.distance(g, b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        best.min(self.distance(g, 0.5 * (lo + hi)))
    }
}

#[test]
fn an_external_tooth_is_exactly_what_the_rack_leaves() -> crate::Result<()> {
    for (name, e) in cases() {
        let rack = Rack::of(&e);
        let outline = e.outline(2e-3)?;
        let window = det::acos(e.root_radius()? / e.tip_radius()?) + 0.2;
        let tolerance = 2e-8 * e.module;
        for piece in outline.tooth() {
            let points: Vec<Point> = match piece {
                Piece::Curve { points, .. } => points.clone(),
                Piece::Line { .. } | Piece::Spline { .. } => {
                    panic!("{name}: a rack-cut tooth has no such piece")
                }
                Piece::Arc { .. } => {
                    let (start, end) = (piece.start().unwrap(), piece.end().unwrap());
                    let (a0, a1) = (det::atan2(start[1], start[0]), det::atan2(end[1], end[0]));
                    let radius = det::hypot(start[0], start[1]);
                    (0..=8)
                        .map(|i| a0 + (a1 - a0) * f64::from(i) / 8.0)
                        .map(|a| [radius * det::cos(a), radius * det::sin(a)])
                        .collect()
                }
            };
            for g in points {
                let closest = rack.closest(g, window);
                match piece.role() {
                    // The rack never reaches the top of the tooth: that is the blank's own surface.
                    Role::Tip => assert!(
                        closest > -tolerance,
                        "{name}: the rack cuts into the tip ({closest})"
                    ),
                    // Everywhere else the rack touches each point once and never passes through it.
                    role => assert!(
                        closest.abs() < tolerance,
                        "{name}: {role:?} at {g:?} is {closest} from the rack"
                    ),
                }
            }
        }
    }
    Ok(())
}

#[test]
fn what_cannot_be_cut_is_refused() {
    let ok = shaft_of(2.0, 20, 0.0);
    let refused = |e: External| e.outline(1e-3).is_err() && e.pitch_radius().is_err();
    assert!(refused(External { module: 0.0, ..ok }));
    assert!(refused(External {
        module: f64::NAN,
        ..ok
    }));
    assert!(refused(External { teeth: 2, ..ok }));
    assert!(refused(External {
        pressure_angle: 0.0,
        ..ok
    }));
    assert!(refused(External {
        pressure_angle: 89.0,
        ..ok
    }));
    assert!(refused(External {
        tool_tip_radius: 0.6,
        ..ok
    })); // the two rounds of the tool would overlap
    assert!(refused(External {
        tool_tip_radius: 1.3,
        ..ok
    })); // the round would end above the pitch circle
    assert!(refused(External {
        addendum: 4.0,
        ..ok
    })); // pointed long before the tip circle
    assert!(refused(External {
        dedendum: 4.9,
        teeth: 5,
        ..ok
    })); // the tool's tooth is not that long
    assert!(refused(External {
        thinning: 10.0,
        ..ok
    }));
    assert!(refused(External {
        shift: f64::INFINITY,
        ..ok
    }));
    assert!(ok.outline(0.0).is_err() && ok.outline(-1.0).is_err() && ok.outline(f64::NAN).is_err());
    // Finer than a curve can be given with a reasonable count of points
    assert!(ok.outline(1e-13).is_err());
    assert!(ok.half_angle_at(5.0).is_err() && ok.half_angle_at(f64::NAN).is_err());
}

// -------------------------------------------------------------------------------------------------
// Internal teeth
// -------------------------------------------------------------------------------------------------

fn fit() -> Fit {
    Fit {
        side_clearance: 0.06,
        hub_addendum: 0.5,
        tip_clearance: 0.2,
        hub_fillet_radius: 0.3,
        form_clearance: 0.05,
    }
}

#[test]
fn an_internal_outline_is_a_closed_chain_with_round_corners_that_touch() -> crate::Result<()> {
    for rho in [0.3, 0.0, 0.05] {
        let hub = hub_for(
            &spline_shaft(),
            &Fit {
                hub_fillet_radius: rho,
                ..fit()
            },
        )?;
        let outline = hub.outline(1e-7)?;
        check_chain("hub", &outline);
        let (low, high) = outline.radii();
        assert!(
            (low - hub.minor_radius).abs() < 1e-9 && (high - hub.major_radius).abs() < 1e-7,
            "{low} {high}"
        );
        // The rounds leave the flank and reach the root circle without a corner.
        let tooth = outline.tooth();
        for (i, piece) in tooth.iter().enumerate() {
            let Piece::Arc {
                centre,
                radius,
                start,
                sweep,
                role: Role::Fillet,
            } = piece
            else {
                continue;
            };
            let tangent = |angle: f64| {
                [
                    -det::sin(angle) * sweep.signum(),
                    det::cos(angle) * sweep.signum(),
                ]
            };
            let (leaving, arriving) = (tangent(*start), tangent(start + sweep));
            let angle_between = |a: Point, b: Point| {
                det::atan2(a[0] * b[1] - a[1] * b[0], a[0] * b[0] + a[1] * b[1]).abs()
            };
            // The neighbour that is a flank: compare with the flank's own direction there.
            for (neighbour, mine, at_start) in
                [(i.wrapping_sub(1), leaving, true), (i + 1, arriving, false)]
            {
                let Some(Piece::Curve { points, .. }) = tooth.get(neighbour) else {
                    continue;
                };
                let n = points.len();
                let theirs = if at_start {
                    [
                        points[n - 1][0] - points[n - 2][0],
                        points[n - 1][1] - points[n - 2][1],
                    ]
                } else {
                    [points[1][0] - points[0][0], points[1][1] - points[0][1]]
                };
                assert!(
                    angle_between(mine, theirs) < 0.01,
                    "a corner between round and flank"
                );
            }
            // The round touches the root circle from inside.
            assert!((det::hypot(centre[0], centre[1]) + radius - hub.major_radius).abs() < 1e-9);
        }
    }
    Ok(())
}

#[test]
fn a_hub_follows_its_shaft() -> crate::Result<()> {
    let shaft = spline_shaft();
    let hub = hub_for(&shaft, &fit())?;
    assert_eq!(
        (hub.teeth, hub.module, hub.pressure_angle),
        (shaft.teeth, shaft.module, shaft.pressure_angle)
    );
    assert!((hub.space_width - (shaft.tooth_thickness()? + 0.06)).abs() < 1e-12);
    assert!((hub.major_radius - (shaft.tip_radius()? + 0.2)).abs() < 1e-12);
    assert!((hub.minor_radius - (shaft.pitch_radius()? - 0.75)).abs() < 1e-12);
    assert!(hub.minor_radius >= shaft.form_radius()? + 0.05);
    // A hub whose teeth would reach into the shaft's root fillet is refused.
    assert!(hub_for(
        &shaft,
        &Fit {
            hub_addendum: 0.7,
            ..fit()
        }
    )
    .is_err());
    assert!(hub_for(
        &shaft,
        &Fit {
            side_clearance: -0.1,
            ..fit()
        }
    )
    .is_err());
    assert!(hub_for(
        &shaft,
        &Fit {
            hub_fillet_radius: 5.0,
            ..fit()
        }
    )
    .is_err());
    Ok(())
}

#[test]
fn internal_teeth_that_cannot_be_made_are_refused() -> crate::Result<()> {
    let ok = hub_for(&spline_shaft(), &fit())?;
    assert!(ok.outline(1e-3).is_ok());
    assert!(Internal {
        space_width: 0.0,
        ..ok
    }
    .outline(1e-3)
    .is_err());
    assert!(Internal {
        space_width: 100.0,
        ..ok
    }
    .outline(1e-3)
    .is_err());
    assert!(Internal {
        minor_radius: 10.0,
        ..ok
    }
    .outline(1e-3)
    .is_err()); // below the base circle
    assert!(Internal {
        major_radius: ok.minor_radius,
        ..ok
    }
    .outline(1e-3)
    .is_err());
    assert!(Internal {
        fillet_radius: 1.6,
        ..ok
    }
    .outline(1e-3)
    .is_err()); // the rounds would overlap
    assert!(Internal {
        fillet_radius: 1.8,
        ..ok
    }
    .outline(1e-3)
    .is_err()); // nothing of the flank would be left
    assert!(Internal { teeth: 0, ..ok }.outline(1e-3).is_err());
    assert!(Internal {
        module: f64::NAN,
        ..ok
    }
    .pitch_radius()
    .is_err());
    assert!((TAU / 20.0 - ok.outline(1e-3)?.pitch_angle()).abs() < 1e-15);
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// A tooth cut through at its root
// -------------------------------------------------------------------------------------------------

fn cut_through(e: &External) -> bool {
    why(&e.outline(1e-3)) == Some(Why::CutThrough)
}

/// A deeply undercut tooth whose two root fillets would cut through each other is refused, exactly
/// where the rack starts to reach the middle of the tooth: drawn back a little less, the test's own
/// rack never reaches the middle of the tooth; a little more, it passes through it. Undercut teeth
/// that still stand (the cases above) are cut.
#[test]
fn a_tooth_the_rack_would_cut_through_at_its_root_is_refused_exactly_there() -> crate::Result<()> {
    let families = [
        External {
            pressure_angle: 14.5,
            tool_tip_radius: 0.0,
            ..shaft_of(2.0, 6, 0.0)
        },
        External {
            pressure_angle: 14.5,
            ..shaft_of(2.0, 6, 0.0)
        },
        External {
            pressure_angle: 20.0,
            thinning: 0.3,
            dedendum: 1.6,
            ..shaft_of(1.5, 9, 0.0)
        },
        External {
            pressure_angle: 17.5,
            tool_tip_radius: 0.1,
            ..shaft_of(3.0, 5, 0.0)
        },
    ];
    for family in families {
        let at = |shift: f64| External { shift, ..family };
        assert!(at(0.0).outline(1e-3).is_ok(), "{family:?}");
        // From standing to cut through, then halved down to the limit.
        let mut through = 0.0;
        for _ in 0..40 {
            through -= 0.05;
            if cut_through(&at(through)) {
                break;
            }
        }
        assert!(cut_through(&at(through)), "{family:?}");
        let mut standing = through + 0.05;
        assert!(at(standing).outline(1e-3).is_ok(), "{family:?}");
        for _ in 0..50 {
            let mid = 0.5 * (standing + through);
            if cut_through(&at(mid)) {
                through = mid;
            } else {
                standing = mid;
            }
        }
        let (kept, lost) = (at(standing + 1e-3), at(standing - 1e-3));
        assert!(
            kept.outline(1e-3).is_ok() && cut_through(&lost),
            "{family:?}"
        );
        // Where the two fillets of the kept tooth come closest to the middle of the tooth.
        let outline = kept.outline(1e-6)?;
        let Piece::Curve { points, .. } = &outline.tooth()[4] else {
            panic!("the upper fillet")
        };
        let nearest = points
            .iter()
            .min_by(|a, b| det::atan2(a[1], a[0]).total_cmp(&det::atan2(b[1], b[0])))
            .unwrap();
        let radius = det::hypot(nearest[0], nearest[1]);
        let deepest = |e: &External| {
            let rack = Rack::of(e);
            let window =
                det::acos((e.root_radius().unwrap_or(radius * 0.5) / radius).min(1.0)) + 0.3;
            (0..=100)
                .map(|i| radius * (0.9 + 0.2 * f64::from(i) / 100.0))
                .map(|along| rack.closest([along, 0.0], window))
                .fold(f64::INFINITY, f64::min)
        };
        let lost_root =
            family.module * (f64::from(family.teeth) / 2.0 - (family.dedendum - lost.shift));
        assert!(
            deepest(&kept) > 0.0,
            "{family:?}: the rack reaches the middle of a tooth that is kept"
        );
        let lost_rack = Rack::of(&lost);
        let window = det::acos((lost_root.max(1e-3) / radius).min(1.0)) + 0.3;
        let through_the_middle = (0..=100)
            .map(|i| radius * (0.9 + 0.2 * f64::from(i) / 100.0))
            .map(|along| lost_rack.closest([along, 0.0], window))
            .fold(f64::INFINITY, f64::min);
        assert!(
            through_the_middle < 0.0,
            "{family:?}: the rack never reaches the middle of a refused tooth"
        );
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// Outlines that would cross themselves
// -------------------------------------------------------------------------------------------------

/// Whether the chord from `p` to `q` crosses the chord from `u` to `v`, each passing from one side
/// of the other to the other side.
fn crosses(p: Point, q: Point, u: Point, v: Point) -> bool {
    let side = |f: Point, t: Point, o: Point| {
        (t[0] - f[0]) * (o[1] - f[1]) - (t[1] - f[1]) * (o[0] - f[0])
    };
    side(u, v, p) * side(u, v, q) < 0.0 && side(p, q, u) * side(p, q, v) < 0.0
}

/// How many pairs of chords of the closed chain cross: every chord against every other but its
/// neighbours, the chords sorted along x so that only those whose spans overlap are compared.
fn self_crossings(points: &[Point]) -> usize {
    let n = points.len();
    let chord = |i: usize| (points[i], points[(i + 1) % n]);
    let mut spans: Vec<(f64, f64, usize)> = (0..n)
        .map(|i| {
            let (a, b) = chord(i);
            (a[0].min(b[0]), a[0].max(b[0]), i)
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut count = 0;
    for (k, &(_, high, i)) in spans.iter().enumerate() {
        for &(low, _, j) in &spans[k + 1..] {
            if low > high {
                break;
            }
            let (i, j) = (i.min(j), i.max(j));
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let ((p, q), (u, v)) = (chord(i), chord(j));
            if crosses(p, q, u, v) {
                count += 1;
            }
        }
    }
    count
}

fn crossing(outline: &Outline) -> usize {
    self_crossings(&outline.points())
}

/// The smallest gap found while a pair was turned; infinite when the parts never came within reach.
fn least(gaps: &Gaps) -> f64 {
    gaps.least.map_or(f64::INFINITY, |g| g.distance)
}

#[test]
fn an_undercut_tooth_cut_through_at_its_root_is_refused_and_no_accepted_outline_crosses_itself() {
    // A shaft of six teeth drawn back so far that its outline would cross itself in twelve places
    // at the root: refused with the reason.
    let twelve_crossings = External {
        module: 2.0,
        teeth: 6,
        pressure_angle: 14.5,
        addendum: 1.0,
        dedendum: 1.25,
        shift: -0.6,
        tool_tip_radius: 0.0,
        thinning: 0.0,
    };
    for tool_tip_radius in [0.0, 0.3] {
        for thinning in [0.0, 0.1] {
            let e = External {
                tool_tip_radius,
                thinning,
                ..twelve_crossings
            };
            assert_eq!(why(&e.outline(2e-3)), Some(Why::CutThrough), "{e:?}");
        }
    }
    // Ordinary proportions, few teeth, drawn back: whatever is accepted stands.
    let mut accepted = 0;
    for pressure_angle in [14.5, 17.5, 20.0] {
        for teeth in [6, 8, 10, 12] {
            for shift in [-0.6, -0.4, -0.2, 0.0] {
                for tool_tip_radius in [0.0, 0.3] {
                    for thinning in [0.0, 0.1] {
                        let e = External {
                            teeth,
                            pressure_angle,
                            shift,
                            tool_tip_radius,
                            thinning,
                            ..twelve_crossings
                        };
                        if let Ok(outline) = e.outline(2e-3) {
                            accepted += 1;
                            assert_eq!(crossing(&outline), 0, "{e:?}");
                        }
                    }
                }
            }
        }
    }
    assert!(accepted > 150, "{accepted}");
    // Two random sweeps: any numbers, and plausible ones.
    let mut dice = Dice(99);
    let mut pick =
        |low: f64, high: f64| low + (high - low) * (dice.roll() % 10_000) as f64 / 10_000.0;
    let mut accepted = 0;
    for i in 0..3000 {
        let wide = i % 3 == 0;
        let e = External {
            module: pick(0.5, 4.0),
            teeth: if wide {
                3 + (pick(0.0, 40.0) as u32)
            } else {
                8 + (pick(0.0, 40.0) as u32)
            },
            pressure_angle: if wide {
                pick(8.0, 40.0)
            } else {
                pick(14.0, 25.0)
            },
            addendum: pick(0.3, 1.8),
            dedendum: pick(0.5, 2.0),
            shift: if wide {
                pick(-1.5, 2.0)
            } else {
                pick(-0.5, 1.0)
            },
            tool_tip_radius: pick(0.0, 0.8),
            thinning: if wide {
                pick(0.0, 0.4)
            } else {
                pick(0.0, 0.15)
            },
        };
        if let Ok(outline) = e.outline(0.01 * e.module) {
            accepted += 1;
            assert_eq!(crossing(&outline), 0, "{e:?}");
        }
    }
    assert!(accepted > 500, "{accepted}");
}

// -------------------------------------------------------------------------------------------------
// A hub whose root rounds reach the shaft's tips
// -------------------------------------------------------------------------------------------------

/// `hub_for` refuses root rounds so large for the tip clearance that they reach the shaft's tip
/// corners, exactly there: just smaller, the spline pair is clear wherever in its side play the hub
/// stands; larger, the hub and shaft overlap once the hub is turned on the shaft as far as the side
/// clearance lets it. Small rounds leave the side play asked for.
#[test]
fn a_hub_whose_root_rounds_would_reach_the_shafts_tips_is_refused_exactly_there(
) -> crate::Result<()> {
    let shaft = spline_shaft();
    for (side, tip) in [(0.0, 0.05), (0.02, 0.05), (0.06, 0.1)] {
        let fit = |round: f64| Fit {
            side_clearance: side,
            hub_addendum: 0.5,
            tip_clearance: tip,
            hub_fillet_radius: round,
            form_clearance: 0.05,
        };
        let (mut fits, mut reaches) = (0.0, 1.0);
        assert!(hub_for(&shaft, &fit(fits)).is_ok() && hub_for(&shaft, &fit(reaches)).is_err());
        for _ in 0..50 {
            let mid = 0.5 * (fits + reaches);
            if hub_for(&shaft, &fit(mid)).is_ok() {
                fits = mid;
            } else {
                reaches = mid;
            }
        }
        let play = side * det::cos(30f64.to_radians()) / 2.0;
        let small = spline_pair(&shaft, &fit(0.2 * fits), 1e-6)?.through_a_cycle(4, 0.3)?;
        assert!(
            (least(&small) - play).abs() < 2e-5,
            "{side} {tip}: {small:?}"
        );
        // The hub turns on the shaft by half the side clearance, as an arc on the pitch circle,
        // until the flanks touch.
        let turn = side / 2.0 / shaft.pitch_radius()?;
        let kept_pair = spline_pair(&shaft, &fit(fits - 2e-3), 1e-6)?;
        let kept = kept_pair.through_a_cycle(4, 0.3)?;
        assert!(
            least(&kept) > -2e-5 && least(&kept) <= play + 2e-5,
            "{side} {tip}: {kept:?}"
        );
        for by in [turn, -turn] {
            let mut turned = kept_pair.clone();
            turned.second_start += by;
            let found = least(&turned.through_a_cycle(4, 0.3)?);
            assert!(
                found > -2e-5,
                "{side} {tip}: turned by {by}, the kept hub overlaps by {found}"
            );
        }
        let refused = hub_for(&shaft, &fit(fits + 2e-3));
        assert_eq!(why(&refused), Some(Why::HubRoundsReachTips), "{refused:?}");
        // A hub made by hand with rounds a little past the limit, turned as far as it goes: the
        // rounds are in the shaft's tip corners.
        let hub = Internal {
            fillet_radius: fits + 0.02,
            ..hub_for(&shaft, &fit(fits - 2e-3))?
        };
        let mut pair = Pair {
            second: Part {
                outline: hub.outline(1e-6)?,
                material: Material::Outside,
                axis: [0.0, 0.0],
            },
            ..spline_pair(&shaft, &fit(fits - 2e-3), 1e-6)?
        };
        pair.second_start += turn;
        let found = least(&pair.through_a_cycle(4, 0.3)?);
        assert!(
            found < -1e-5,
            "{side} {tip}: rounds past the limit, yet the least gap is {found}"
        );
    }
    Ok(())
}

// -------------------------------------------------------------------------------------------------
// The root fillets proved apart
// -------------------------------------------------------------------------------------------------

/// The corpus of the proofs' oracles: module 1, addendum 1, dedendum 1.25, no thinning, and every
/// count of teeth, pressure angle, shift and tool tip radius of the lists below (880 teeth).
fn corpus() -> Vec<External> {
    let mut out = Vec::new();
    for teeth in [6, 7, 8, 9, 10, 12, 15, 20, 30, 50, 80] {
        for pressure_angle in [14.5, 20.0, 25.0, 30.0] {
            for shift in [-0.8, -0.4, 0.0, 0.4, 0.8] {
                for tool_tip_radius in [0.0, 0.1, 0.25, 0.4] {
                    out.push(External {
                        module: 1.0,
                        teeth,
                        pressure_angle,
                        addendum: 1.0,
                        dedendum: 1.25,
                        shift,
                        tool_tip_radius,
                        thinning: 0.0,
                    });
                }
            }
        }
    }
    out
}

/// The old `Cut::fillet_angles`, letter for letter over the hook's point function: the smallest
/// and the largest angle from the middle of the tooth of the points of its upper root fillet,
/// sampled at 256 steps and refined by thirds around the extreme step.
fn old_fillet_angles(fillet: &Fillet) -> (f64, f64) {
    const STEPS: u32 = 256;
    let angle = |phi: f64| {
        let [x, y] = fillet.point(phi);
        det::atan2(y, x)
    };
    let at = |i: u32| {
        fillet.phi_root + (fillet.phi_join - fillet.phi_root) * f64::from(i) / f64::from(STEPS)
    };
    // The extreme among the steps, then closer between its neighbours (the curve is smooth
    // there).
    let extreme = |sign: f64| {
        let (mut best, mut best_i) = (f64::INFINITY, 0);
        for i in 0..=STEPS {
            let value = sign * angle(at(i));
            if value < best {
                (best, best_i) = (value, i);
            }
        }
        let (mut lo, mut hi) = (at(best_i.saturating_sub(1)), at((best_i + 1).min(STEPS)));
        for _ in 0..100 {
            let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if sign * angle(a) < sign * angle(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        sign * best.min(sign * angle(0.5 * (lo + hi)))
    };
    (extreme(1.0), extreme(-1.0))
}

/// What a test of the root fillets decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Kept,
    CutThrough,
    FilletsCross,
    NotProved,
    Other,
}

/// The proof's decision on a fillet.
fn proved(fillet: &Fillet) -> Decision {
    match fillet.separation() {
        Ok(()) => Decision::Kept,
        Err(TeethFailure::Invalid(Why::CutThrough)) => Decision::CutThrough,
        Err(TeethFailure::Invalid(Why::FilletsCross)) => Decision::FilletsCross,
        Err(TeethFailure::NotProved { what, .. }) if what == FILLET_SEPARATION => {
            Decision::NotProved
        }
        Err(_) => Decision::Other,
    }
}

/// The proof that the root fillets stay apart decides as the old sampling did wherever the old
/// margin (the least angle of the fillet from the middle of the tooth or from the middle of the
/// space) is at least 1e-9, and never leaves such a fillet unproved. Fillets nearer the edge than
/// that are only counted.
#[test]
fn the_fillet_proof_decides_as_the_old_sampling_did() {
    let (mut checked, mut differ, mut unproved, mut near) = (0, 0, 0, 0);
    let mut first = None;
    for e in corpus() {
        // Refused before the fillet's stretch is known: nothing to compare.
        let Ok(fillet) = e.fillet() else { continue };
        checked += 1;
        let (lowest, highest) = old_fillet_angles(&fillet);
        let beta = PI / fillet.z;
        let old = if !(lowest > 0.0) {
            Decision::CutThrough
        } else if !(highest < beta) {
            Decision::FilletsCross
        } else {
            Decision::Kept
        };
        let margin = lowest.min(beta - highest);
        let new = proved(&fillet);
        if !(margin.abs() >= 1e-9) {
            near += 1;
            continue;
        }
        if new == Decision::NotProved {
            unproved += 1;
        } else if new != old {
            differ += 1;
        }
        // The old sampling never leaves a fillet unproved, so a NotProved always differs.
        if new != old {
            first
                .get_or_insert_with(|| format!("{e:?}: old {old:?}, new {new:?}, margin {margin}"));
        }
    }
    assert!(
        checked > 0 && near < 880,
        "checked {checked}, near the edge {near}"
    );
    assert_eq!(
        (differ, unproved),
        (0, 0),
        "near the edge: {near} of {checked}; first: {first:?}"
    );
}

/// Teeth whose root fillet reaches the root circle just short of the middle of the space, by the
/// relative margins 1e-6, 1e-8 and 1e-12 (the rack's tip round chosen so that its centre stands
/// at `pitch/2 (1 - eps)`): the first two are made; the last is made or named unproved, never
/// refused as crossing.
#[test]
fn a_fillet_near_the_edge_is_proved_or_named_unproved() -> crate::Result<()> {
    for (teeth, pressure_angle) in [(24, 20.0), (30, 25.0), (40, 30.0)] {
        for eps in [1e-6, 1e-8, 1e-12] {
            let (m, z) = (1.0, f64::from(teeth));
            let (sin, cos) = det::sin_cos(f64::to_radians(pressure_angle));
            // No shift and no thinning: the tooth is half the pitch thick.
            let half_pitch = PI * m / 2.0;
            let s = half_pitch;
            let depth = 1.25 * m;
            let rho = ((half_pitch * (1.0 - eps) - s / 2.0) * cos - depth * sin) / (1.0 - sin);
            let e = External {
                module: m,
                teeth,
                pressure_angle,
                addendum: 1.0,
                dedendum: 1.25,
                shift: 0.0,
                tool_tip_radius: rho / m,
                thinning: 0.0,
            };
            // The fillet reaches the root circle at pi/z (1 - eps), as asked.
            let fillet = lift(e.fillet())?;
            let margin = (PI / z - fillet.phi_root) / (PI / z);
            assert!(
                (margin / eps - 1.0).abs() < 0.05,
                "{teeth} {pressure_angle} {eps}: margin {margin}"
            );
            let made = e.outline(1e-4);
            if eps >= 1e-8 {
                assert!(
                    made.is_ok(),
                    "{teeth} {pressure_angle} {eps}: {:?}",
                    made.err()
                );
            } else {
                assert!(
                    made.is_ok() || not_proved(&made) == Some(FILLET_SEPARATION),
                    "{teeth} {pressure_angle} {eps}: {:?}",
                    made.err()
                );
            }
        }
    }
    Ok(())
}

/// Whether `value` lies in `range`, allowing `slack`.
fn holds(range: Ivl, value: f64, slack: f64) -> bool {
    range.lo - slack <= value && value <= range.hi + slack
}

/// The first and second derivative of `curve` at `t` by central differences, each extrapolated
/// from a step and its half (Richardson), at the step among `scale` times 1e-1 to 1e-4 whose two
/// estimates agree best; with each, its own error: that disagreement plus the rounding of the
/// points divided by the step.
fn derivatives(curve: &dyn Fn(f64) -> Point, t: f64, scale: f64) -> [(Point, f64); 2] {
    let p = curve(t);
    let size = 1.0 + p[0].abs() + p[1].abs();
    let mut best = [([f64::NAN; 2], f64::INFINITY); 2];
    for h in [1e-1, 1e-2, 1e-3, 1e-4].map(|k| k * scale) {
        let (a, b) = (curve(t + h), curve(t - h));
        let (c, d) = (curve(t + h / 2.0), curve(t - h / 2.0));
        let first = |i: usize| {
            let (wide, narrow) = ((a[i] - b[i]) / (2.0 * h), (c[i] - d[i]) / h);
            ((4.0 * narrow - wide) / 3.0, (narrow - wide).abs())
        };
        let second = |i: usize| {
            let wide = (a[i] - 2.0 * p[i] + b[i]) / (h * h);
            let narrow = (c[i] - 2.0 * p[i] + d[i]) / (h * h / 4.0);
            ((4.0 * narrow - wide) / 3.0, (narrow - wide).abs())
        };
        let (f0, f1, s0, s1) = (first(0), first(1), second(0), second(1));
        let found = [
            ([f0.0, f1.0], f0.1.max(f1.1) + 1e-14 * size / h),
            ([s0.0, s1.0], s0.1.max(s1.1) + 1e-13 * size / (h * h)),
        ];
        for (kept, new) in best.iter_mut().zip(found) {
            if new.1 < kept.1 {
                *kept = new;
            }
        }
    }
    best
}

/// The enclosures the fillet's proof stands on hold the fillet: for 20 teeth of the corpus and 64
/// stretches of each fillet (some from its first end, some to its last, some the whole), the
/// enclosures of both tests (the centred form, and the end forms where the stretch has an end of
/// the fillet) contain the tests' values at 1,000 points of the stretch, and the fillet's bend
/// (its first and second derivative boxes) contains the central differences there, with a slack
/// of 1e-6 of the box's size plus 1e-9 and the differences' own estimated error.
#[test]
fn the_fillet_enclosures_hold_every_dense_point() {
    let fillets: Vec<Fillet> = corpus().iter().filter_map(|e| e.fillet().ok()).collect();
    let every = (fillets.len() / 20).max(1);
    let mut dice = Dice(5);
    let mut teeth = 0;
    for fillet in fillets.iter().step_by(every).take(20) {
        teeth += 1;
        let (lo, hi) = (
            fillet.phi_root.min(fillet.phi_join),
            fillet.phi_root.max(fillet.phi_join),
        );
        let beta = PI / fillet.z;
        let curve = |phi: f64| fillet.point(phi);
        for k in 0..64 {
            let mut pick = || lo + (hi - lo) * (dice.roll() % 1_000_001) as f64 / 1e6;
            let (a, b) = (pick(), pick());
            let (p0, p1) = match k % 8 {
                0 => (lo, a.max(b)),
                1 => (a.min(b), hi),
                2 => (lo, hi),
                _ => (a.min(b), a.max(b)),
            };
            if p1 <= p0 {
                continue;
            }
            let (start, end) = (p0 == lo, p1 == hi);
            let above = fillet.enclosure(Wedge::AboveMiddle, p0, p1, start, end);
            let below = fillet.enclosure(Wedge::BelowSpace, p0, p1, start, end);
            let bend = fillet.bend(p0, p1);
            for j in 0..1000 {
                let t = p0 + (p1 - p0) * f64::from(j) / 999.0;
                let [x, y] = curve(t);
                let slack = 1e-12 * (1.0 + x.abs() + y.abs());
                let across = det::sin(beta) * x - det::cos(beta) * y;
                assert!(
                    holds(above, y, slack) && holds(below, across, slack),
                    "{fillet:?} [{p0}, {p1}] at {t}: {y} in {above:?}, {across} in {below:?}"
                );
                let scale = (det::hypot(fillet.r * t - fillet.uc, fillet.vc) / fillet.r).min(1.0);
                let [(first, first_error), (second, second_error)] = derivatives(&curve, t, scale);
                let fits = |i: usize| {
                    let (f, s) = (bend.first[i], bend.second[i]);
                    holds(f, first[i], 1e-6 * f.mag() + 1e-9 + first_error)
                        && holds(s, second[i], 1e-6 * s.mag() + 1e-9 + second_error)
                };
                assert!(
                    fits(0) && fits(1),
                    "{fillet:?} [{p0}, {p1}] at {t}: {first:?} ({first_error}) and {second:?} \
                     ({second_error}) against {bend:?}"
                );
            }
        }
    }
    assert_eq!(teeth, 20, "of {} fillets", fillets.len());
}

/// The deeply undercut teeth of the cut-through corpus are refused for the same reason, in the
/// same words, by the function asked, and the proof alone gives that reason.
#[test]
fn a_deeply_undercut_tooth_is_refused_in_the_same_words() {
    let twelve_crossings = External {
        module: 2.0,
        teeth: 6,
        pressure_angle: 14.5,
        addendum: 1.0,
        dedendum: 1.25,
        shift: -0.6,
        tool_tip_radius: 0.0,
        thinning: 0.0,
    };
    for tool_tip_radius in [0.0, 0.3] {
        for thinning in [0.0, 0.1] {
            for tolerance in [2e-3, 1e-5] {
                let e = External {
                    tool_tip_radius,
                    thinning,
                    ..twelve_crossings
                };
                let refused = e.outline(tolerance);
                assert_eq!(why(&refused), Some(Why::CutThrough), "{e:?}");
                assert_eq!(
                    failure(&refused)
                        .as_ref()
                        .map(ToString::to_string)
                        .as_deref(),
                    Some(
                        "the undercut would cut the tooth through at its root: its two root \
                         fillets would meet"
                    ),
                    "{e:?}"
                );
                assert_eq!(
                    operation(&refused),
                    Some("teeth::involute::External::outline"),
                    "{e:?}"
                );
                assert_eq!(
                    typed_why(&e.fillet().and_then(|f| f.separation())),
                    Some(Why::CutThrough),
                    "{e:?}"
                );
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// The chords of flanks and fillets proved
// -------------------------------------------------------------------------------------------------

/// What the chord oracle counts over its curves.
#[derive(Debug, Default)]
struct Chains {
    /// Curves compared (those the old sampling made).
    curves: usize,
    /// Curves where a proved chain has fewer points than the old one.
    coarser: usize,
    /// Curves where a proved chain has more points than its limit, 4 old + 16.
    too_many: usize,
    /// Curves the old sampling made and the proved one refused.
    refused: usize,
    /// Curves whose proved chain is not the old one.
    grew: usize,
    /// The largest growth, proved points over old points.
    largest: f64,
    /// The first curve that broke a rule.
    first: Option<String>,
}

/// The points of the curve a chain is checked against.
const DENSE: usize = 20_000;

/// How far `p` is from the chord between `a` and `b`.
fn off_chord(p: Point, a: Point, b: Point) -> f64 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let length2 = ex * ex + ey * ey;
    if length2 > 0.0 {
        let t = (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / length2).clamp(0.0, 1.0);
        dist(p, [a[0] + t * ex, a[1] + t * ey])
    } else {
        dist(p, a)
    }
}

/// The distance from `p` to the polyline `line`, looking at its chords from `*from` on, as long as
/// a nearer one may follow, and moving `*from` to the nearest: points taken along the curve in
/// order meet the chords of a chain of the same curve in order.
fn to_polyline(p: Point, line: &[Point], from: &mut usize) -> f64 {
    const AHEAD: usize = 16;
    let chords = line.len().saturating_sub(1);
    if chords == 0 {
        return line.first().map_or(f64::INFINITY, |&a| dist(p, a));
    }
    let (mut best, mut best_at) = (f64::INFINITY, *from);
    let mut j = *from;
    while j < chords && j < best_at + AHEAD {
        let d = off_chord(p, line[j], line[j + 1]);
        if d < best {
            (best, best_at) = (d, j);
        }
        j += 1;
    }
    *from = best_at;
    best
}

/// Asserts that `chain`, made for the curve `at` over `[from, to]`, is within `tolerance` of it
/// both ways, against `DENSE` points of the curve: each dense point within the tolerance of the
/// chain, and the points at a quarter, a half and three quarters of each chord within the
/// tolerance of the dense points' polyline plus that polyline's own sag; each allowing for the
/// rounding of the points.
fn assert_within(
    name: &str,
    chain: &[Point],
    at: &dyn Fn(f64) -> Point,
    from: f64,
    to: f64,
    tolerance: f64,
) {
    let dense: Vec<Point> = (0..=DENSE)
        .map(|i| at(from + (to - from) * i as f64 / DENSE as f64))
        .collect();
    let size = dense.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
    let slack = 1e-12 * (1.0 + size);
    // A chord's sag is about an eighth of the second difference of its neighbours; a quarter is
    // taken.
    let sag = dense
        .windows(3)
        .map(|w| {
            det::hypot(
                w[0][0] - 2.0 * w[1][0] + w[2][0],
                w[0][1] - 2.0 * w[1][1] + w[2][1],
            )
        })
        .fold(0.0_f64, f64::max)
        / 4.0;
    let mut k = 0;
    for (i, &p) in dense.iter().enumerate() {
        let d = to_polyline(p, chain, &mut k);
        assert!(
            d <= tolerance + slack,
            "{name}: the dense point {i} is {d} from the chain (tolerance {tolerance})"
        );
    }
    let mut k = 0;
    for (i, pair) in chain.windows(2).enumerate() {
        for s in [0.25, 0.5, 0.75] {
            let c = [
                pair[0][0] + s * (pair[1][0] - pair[0][0]),
                pair[0][1] + s * (pair[1][1] - pair[0][1]),
            ];
            let d = to_polyline(c, &dense, &mut k);
            assert!(
                d <= tolerance + sag + slack,
                "{name}: chord {i} at {s} is {d} from the curve (tolerance {tolerance}, sag {sag})"
            );
        }
    }
}

/// One curve of the chord oracle: the old sampling against the proved one, counted, and the proved
/// chain checked against dense points of the curve.
fn compare_chains(
    counts: &mut Chains,
    name: &str,
    at: &dyn Fn(f64) -> Point,
    off: &dyn Fn(f64, Point) -> f64,
    bend: &dyn Fn(f64, f64) -> Bend,
    from: f64,
    to: f64,
    tolerance: f64,
) {
    let Ok(old) = old_sampled(at, from, to, tolerance) else {
        return;
    };
    counts.curves += 1;
    let proved = match sampled(at, off, bend, from, to, tolerance) {
        Ok(proved) => proved,
        Err(e) => {
            counts.refused += 1;
            counts
                .first
                .get_or_insert_with(|| format!("{name}: refused, {e}"));
            return;
        }
    };
    let n = old.len();
    if proved.len() < n {
        counts.coarser += 1;
        counts
            .first
            .get_or_insert_with(|| format!("{name}: {} points, old {n}", proved.len()));
    }
    if proved.len() > 4 * n + 16 {
        counts.too_many += 1;
        counts
            .first
            .get_or_insert_with(|| format!("{name}: {} points, old {n}", proved.len()));
    }
    if proved != old {
        counts.grew += 1;
    }
    counts.largest = counts.largest.max(proved.len() as f64 / n as f64);
    assert_within(name, &proved, at, from, to, tolerance);
}

/// Every chord of an external tooth's fillet and flank and of a hub's flank is proved within the
/// tolerance, and the proved chains are never coarser than the old sampling's (its probes still
/// nominate, so every old split is made) and grow by at most the limits: over the 880 teeth of the
/// corpus and the hubs of 40 of them, at 1e-3, 1e-5 and 1e-7 of the module. No curve is refused.
/// Each proved chain is checked against 20,000 points of its curve.
#[test]
fn the_flank_and_fillet_chains_are_proved_and_never_coarser() -> crate::Result<()> {
    const TOLERANCES: [f64; 3] = [1e-3, 1e-5, 1e-7];
    let mut counts = Chains::default();
    let mut hubs = Vec::new();
    for e in corpus() {
        let Ok(flank) = e.flank() else { continue };
        let fillet = lift(e.fillet())?;
        if hubs.len() < 40 {
            if let Ok(hub) = hub_for(&e, &fit()) {
                hubs.push(hub);
            }
        }
        for tolerance in TOLERANCES {
            compare_chains(
                &mut counts,
                &format!("the fillet of {e:?} at {tolerance}"),
                &|phi| fillet.point(phi),
                &|phi, p| fillet.off(phi, p),
                &|lo, hi| fillet.bend(lo, hi),
                fillet.phi_root,
                fillet.phi_join,
                tolerance,
            );
            compare_chains(
                &mut counts,
                &format!("the flank of {e:?} at {tolerance}"),
                &|v| flank.point(v),
                &|v, p| flank.off(v, p),
                &|lo, hi| flank.bend(lo, hi),
                flank.from,
                flank.to,
                tolerance,
            );
        }
    }
    assert_eq!(hubs.len(), 40, "hubs of the corpus");
    for hub in &hubs {
        let flank = lift(hub.flank())?;
        for tolerance in TOLERANCES {
            compare_chains(
                &mut counts,
                &format!("the flank of {hub:?} at {tolerance}"),
                &|roll| flank.point(roll),
                &|roll, p| flank.off(roll, p),
                &|lo, hi| flank.bend(lo, hi),
                flank.from,
                flank.to,
                tolerance,
            );
        }
    }
    assert!(counts.curves > 0, "{counts:?}");
    assert_eq!(
        (counts.coarser, counts.too_many, counts.refused),
        (0, 0, 0),
        "{counts:?}"
    );
    Ok(())
}

/// The proved distance of a fillet's computed point from its true one covers the fillet's
/// conditioning. Where the centre of the tool's tip round lies a hair below the line of rolling,
/// the unit vector from the point of rolling contact to that centre is ill conditioned, and the
/// rounding of the rolling `r phi` moves the computed point far more than a count of operations
/// on its size would allow: the enclosure the point is measured against widens with it. The
/// reference rolls with one rounding (`mul_add`), so its own error there is far below the
/// difference measured.
#[test]
fn a_fillet_close_to_the_line_of_rolling_counts_its_conditioning() {
    let fillet = |vc: f64| Fillet {
        r: 10.0,
        uc: 0.4,
        vc,
        rho: 0.3,
        z: 20.0,
        phi_root: 0.0,
        phi_join: 0.1,
    };
    // The point as `Fillet::point` computes it, with the rolling rounded once instead of twice.
    let reference = |f: &Fillet, phi: f64| -> Point {
        let (du, dv) = (f.r.mul_add(phi, -f.uc), -f.vc);
        let len = det::hypot(du, dv);
        let (qu, qv) = (f.uc - f.rho * du / len, f.vc - f.rho * dv / len);
        let (sn, cs) = det::sin_cos(phi);
        let along = qu - f.r * phi;
        [(f.r + qv) * cs - along * sn, (f.r + qv) * sn + along * cs]
    };
    // Angles of rolling about the one where the contact passes under the centre (`r phi = uc`).
    let (close, steps) = (fillet(-1e-9), 200_i32);
    let angle = |k: i32| 0.04 + 2e-11 * f64::from(k);
    let mut worst = 0.0_f64;
    for k in -steps..=steps {
        let phi = angle(k);
        let (p, q) = (close.point(phi), reference(&close, phi));
        let moved = det::hypot(p[0] - q[0], p[1] - q[1]);
        let proved = close.off(phi, p);
        assert!(
            moved <= proved + 1e-13,
            "at {phi}: moved {moved:e}, proved {proved:e}"
        );
        worst = worst.max(moved);
    }
    // A fillet well below the line, whose sizes are no smaller, has its points proved within a
    // few parts in 1e15 of its size; the other's points really move orders of magnitude more.
    let plain = fillet(-0.9);
    let without = plain.off(angle(0), plain.point(angle(0)));
    assert!(without > 0.0 && without < 1e-12, "{without:e}");
    assert!(
        worst > 1e3 * without,
        "moved {worst:e}, well placed {without:e}"
    );
    // With the centre on the line of rolling, under the point of contact, nothing is proved.
    let on_line = fillet(0.0);
    assert_eq!(
        on_line.off(angle(0), on_line.point(angle(0))),
        f64::INFINITY
    );
}
