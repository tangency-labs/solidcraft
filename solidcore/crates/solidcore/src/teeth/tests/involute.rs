//! Involute splines, the shaft's teeth and the hub's tooth spaces, checked against their definition
//! rather than against their formulas: a shaft's tooth must be exactly what the rack-shaped tool
//! leaves. No outline that is accepted crosses itself, and a hub whose root rounds would reach the
//! shaft's tips is refused exactly there: each shaft and hub the functions accept is turned and
//! must run clear; each they refuse for an overlap is put together by hand, turned, and must
//! overlap.

use std::f64::consts::{PI, TAU};

use super::common::Dice;
use crate::teeth::involute::{hub_for, inv, spline_pair, External, Fit, Internal};
use crate::teeth::outline::rotated;
use crate::teeth::{Error, Gaps, Material, Outline, Pair, Part, Piece, Point, Role};

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
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[test]
fn the_circles_of_a_plain_shaft() -> Result<(), Error> {
    let g = shaft_of(2.0, 20, 0.0);
    assert!((g.pitch_radius()? - 20.0).abs() < 1e-12);
    assert!((g.base_radius()? - 20.0 * 20f64.to_radians().cos()).abs() < 1e-12);
    assert!((g.tip_radius()? - 22.0).abs() < 1e-12);
    assert!((g.root_radius()? - 17.5).abs() < 1e-12);
    assert!((g.tooth_thickness()? - PI).abs() < 1e-12);
    // A shift moves tip and root and thickens the tooth.
    let shifted = shaft_of(2.0, 20, 0.5);
    assert!((shifted.tip_radius()? - 23.0).abs() < 1e-12);
    assert!((shifted.root_radius()? - 18.5).abs() < 1e-12);
    assert!((shifted.tooth_thickness()? - (PI + 2.0 * 20f64.to_radians().tan())).abs() < 1e-12);
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
fn every_outline_is_a_closed_chain_between_root_and_tip() -> Result<(), Error> {
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
fn the_flank_is_the_involute_of_the_base_circle() -> Result<(), Error> {
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
                let radius = x.hypot(y);
                assert!(
                    radius >= rb * (1.0 - 1e-12),
                    "{name}: a flank point below the base circle"
                );
                let at_radius = (rb / radius).min(1.0).acos();
                let expected = half_at_pitch + inv(e.pressure_angle.to_radians()) - inv(at_radius);
                assert!(
                    (y.atan2(x).abs() - expected).abs() < 1e-11,
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
fn the_fillet_runs_into_the_flank_smoothly_unless_the_tooth_is_undercut() -> Result<(), Error> {
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
        let turn = (into[0] * out_of[1] - into[1] * out_of[0])
            .atan2(into[0] * out_of[0] + into[1] * out_of[1])
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
        let (sin, cos) = e.pressure_angle.to_radians().sin_cos();
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
            (u - c[0] - t * dir[0]).hypot(v - c[1] - t * dir[1])
        };
        let between =
            (u - u.clamp(self.left[0], self.right[0].max(self.left[0]))).hypot(v - self.left[1]);
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
        let (sn, cs) = phi.sin_cos();
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
        let middle = g[1].atan2(g[0]);
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
fn an_external_tooth_is_exactly_what_the_rack_leaves() -> Result<(), Error> {
    for (name, e) in cases() {
        let rack = Rack::of(&e);
        let outline = e.outline(2e-3)?;
        let window = (e.root_radius()? / e.tip_radius()?).acos() + 0.2;
        let tolerance = 2e-8 * e.module;
        for piece in outline.tooth() {
            let points: Vec<Point> = match piece {
                Piece::Curve { points, .. } => points.clone(),
                Piece::Line { .. } | Piece::Spline { .. } => {
                    panic!("{name}: a rack-cut tooth has no such piece")
                }
                Piece::Arc { .. } => {
                    let (start, end) = (piece.start().unwrap(), piece.end().unwrap());
                    let (a0, a1) = (start[1].atan2(start[0]), end[1].atan2(end[0]));
                    let radius = start[0].hypot(start[1]);
                    (0..=8)
                        .map(|i| a0 + (a1 - a0) * f64::from(i) / 8.0)
                        .map(|a| [radius * a.cos(), radius * a.sin()])
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
fn an_internal_outline_is_a_closed_chain_with_round_corners_that_touch() -> Result<(), Error> {
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
            let tangent =
                |angle: f64| [-angle.sin() * sweep.signum(), angle.cos() * sweep.signum()];
            let (leaving, arriving) = (tangent(*start), tangent(start + sweep));
            let angle_between = |a: Point, b: Point| {
                (a[0] * b[1] - a[1] * b[0])
                    .atan2(a[0] * b[0] + a[1] * b[1])
                    .abs()
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
            assert!((centre[0].hypot(centre[1]) + radius - hub.major_radius).abs() < 1e-9);
        }
    }
    Ok(())
}

#[test]
fn a_hub_follows_its_shaft() -> Result<(), Error> {
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
fn internal_teeth_that_cannot_be_made_are_refused() -> Result<(), Error> {
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

const CUT_THROUGH: &str =
    "the undercut would cut the tooth through at its root: its two root fillets would meet";

fn cut_through(e: &External) -> bool {
    matches!(e.outline(1e-3), Err(Error::Invalid { why }) if why == CUT_THROUGH)
}

/// A deeply undercut tooth whose two root fillets would cut through each other is refused, exactly
/// where the rack starts to reach the middle of the tooth: drawn back a little less, the test's own
/// rack never reaches the middle of the tooth; a little more, it passes through it. Undercut teeth
/// that still stand (the cases above) are cut.
#[test]
fn a_tooth_the_rack_would_cut_through_at_its_root_is_refused_exactly_there() -> Result<(), Error> {
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
            .min_by(|a, b| a[1].atan2(a[0]).total_cmp(&b[1].atan2(b[0])))
            .unwrap();
        let radius = nearest[0].hypot(nearest[1]);
        let deepest = |e: &External| {
            let rack = Rack::of(e);
            let window = (e.root_radius().unwrap_or(radius * 0.5) / radius)
                .min(1.0)
                .acos()
                + 0.3;
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
        let window = (lost_root.max(1e-3) / radius).min(1.0).acos() + 0.3;
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
            assert!(
                matches!(e.outline(2e-3), Err(Error::Invalid { why }) if why.contains("cut the tooth through")),
                "{e:?}"
            );
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
) -> Result<(), Error> {
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
        let play = side * 30f64.to_radians().cos() / 2.0;
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
        assert!(
            matches!(refused, Err(Error::Invalid { why }) if why.contains("tip corners")),
            "{refused:?}"
        );
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
