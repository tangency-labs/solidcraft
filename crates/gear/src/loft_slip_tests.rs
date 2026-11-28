//! Every loft the engine builds must survive the kernel's section alignment (it restarts each
//! section at the segment that best lines up with the one before, `loft_numbered_loops`): if a
//! section is matched a few segments off its partner, the loft's walls cut across the teeth.
//! [`crate::hypoid::slips`] copies that rule; this runs it on every kind's lofts, including the
//! spiral tapers the engine lofts through [`crate::plan::taper_sections`] sections.

use crate::geom::Outline;
use crate::plan::{Kind, Spec, Step, plan, taper_turn};

/// The sections the engine lofts for a step, if it lofts it.
fn lofted(step: &Step) -> Option<Vec<Outline>> {
    match step {
        Step::Loft { sections, .. } => Some(sections.iter().map(|(o, _)| o.clone()).collect()),
        Step::Taper { outline, twist, scale, .. } if twist.abs() > 1e-12 => Some({
            let n = crate::plan::taper_sections(outline, *twist, *scale);
            (0..n)
                .map(|i| {
                    let t = i as f64 / (n - 1) as f64;
                    let k = 1.0 + (scale - 1.0) * t;
                    outline.scaled(k).rotated(taper_turn(*twist, *scale, k))
                })
                .collect()
        }),
        Step::Taper { outline, scale, .. } => Some(vec![outline.clone(), outline.scaled(*scale)]),
        // Its rim circle is the same in every section: the toothed loop is what lines up.
        Step::RingTaper { outline, twist, scale, z0, height, rim: _, op } => {
            lofted(&Step::Taper { outline: outline.clone(), z0: *z0, height: *height, twist: *twist, scale: *scale, op: *op })
        }
        Step::Placed { step, .. } => lofted(step),
        _ => None,
    }
}

/// The tooth count a loft's outline repeats with: the first of the set's counts that divides
/// its corner count (whole-tooth restarts are harmless), else none (any restart must be 0).
fn teeth_of(s: &Spec, sections: &[Outline], body: &str) -> usize {
    let corners = sections[0].segs.len();
    // The body's own count where its name gives it ("Bevel z34"): a set's other count may
    // divide the corners too (17 those of a z34 gear), and would allow only restarts by two of
    // its teeth.
    let named = body.rsplit(" z").next().and_then(|t| t.split_whitespace().next()).and_then(|t| t.parse::<usize>().ok());
    if let Some(z) = named.filter(|z| *z > 1 && corners.is_multiple_of(*z)) {
        return z;
    }
    // A bevel planetary set's second sun or ring has the count its cone angle gives (teeth3 0).
    let far = if s.kind == Kind::BevelPlanetary { crate::plan::bevel_planet::geometry(s).map_or(0, |g| g.z_far) } else { 0 };
    // A differential's members: its side gears, pinions, ring and drive pinion.
    let d = &s.diff;
    let diff: Vec<u32> =
        if s.kind == Kind::Differential { vec![d.side_teeth2, d.side_teeth, d.pinion_teeth, d.ring_teeth, d.drive_pinion_teeth] } else { Vec::new() };
    diff.into_iter().chain([far, s.teeth3, s.teeth, s.teeth2]).map(|z| z as usize).find(|z| *z > 1 && corners.is_multiple_of(*z)).unwrap_or(0)
}

fn check(s: &Spec, label: &str, bad: &mut Vec<String>) {
    let Ok(p) = plan(s) else { return };
    for b in p.bodies.iter().chain(p.clearance.iter()) {
        for (i, st) in b.steps.iter().enumerate() {
            // The kernel's loft limit, counted as its spans (a 4-start z40 worm wheel once came
            // to 19200 and the kernel refused it).
            if let Some(sections) = lofted(st)
                && sections.iter().map(crate::plan::loft_curves).sum::<usize>() > crate::plan::LOFT_CURVE_LIMIT
            {
                bad.push(format!(
                    "{label}: {} step {} has {} curves in {} sections: over the kernel's loft limit of 16384",
                    b.name,
                    i + 1,
                    sections.iter().map(crate::plan::loft_curves).sum::<usize>(),
                    sections.len()
                ));
            }
            if let Some(sections) = lofted(st)
                && sections.len() >= 2
                && sections.iter().all(|o| o.segs.len() == sections[0].segs.len())
                && crate::hypoid::slips(&sections, teeth_of(s, &sections, &b.name))
            {
                bad.push(format!("{label}: {} step {} ({} sections)", b.name, i + 1, sections.len()));
            }
        }
    }
}

#[test]
fn no_kernel_loft_slips_in_any_kind() {
    let mut bad = Vec::new();
    for k in Kind::ALL.iter().copied() {
        check(&Spec::defaults_for(k), &format!("{k:?} defaults"), &mut bad);
    }
    // Harder variants of the lofted kinds: steep spirals, other shaft angles, multi-start worms.
    let mut variants = Vec::new();
    for (angle, spiral) in [(90.0, 45.0), (60.0, 35.0), (120.0, 35.0), (90.0, 20.0)] {
        variants.push((
            format!("spiral bevel {angle}°/{spiral}°"),
            Spec { shaft_angle: angle, spiral_angle: spiral, ..Spec::defaults_for(Kind::SpiralBevel) },
        ));
    }
    // Nearly flat cones (built through their back and front cones): 100° to 150° shafts.
    for (label, s) in crate::flat_bevel_tests::flat_sets()
        .into_iter()
        .chain(crate::flat_bevel_tests::own_face_sets())
        .chain(crate::flat_bevel_tests::planetary_sets())
    {
        variants.push((format!("flat {label}"), s));
    }
    for (starts, teeth) in [(1, 30), (2, 41), (4, 40), (4, 48), (1, 80)] {
        variants.push((format!("worm {starts}-start z{teeth}"), Spec { starts, teeth, ..Spec::defaults_for(Kind::Worm) }));
    }
    for angle in [60.0, 75.0, 90.0] {
        variants.push((format!("hypoid {angle}°"), Spec { shaft_angle: angle, ..Spec::defaults_for(Kind::Hypoid) }));
    }
    // Bevel planetary sets: sun, planets, second sun and the hobbed ring, straight and spiral.
    {
        use crate::plan::BevelPlanetMode::{Differential, Ring, Single};
        let bp = Spec::defaults_for(Kind::BevelPlanetary);
        let set = |mode, angle: f64, zs: u32, zp: u32, n: u32, spiral: f64| Spec {
            bevel_planet_mode: mode,
            shaft_angle: angle,
            teeth: zs,
            teeth2: zp,
            planets: n,
            spiral_angle: spiral,
            ..bp.clone()
        };
        for (label, s) in [
            ("bevel planetary diff 90 16:10 x4 spiral 30", set(Differential, 90.0, 16, 10, 4, 30.0)),
            ("bevel planetary diff 90 16:10 x4 spiral 45", set(Differential, 90.0, 16, 10, 4, 45.0)),
            ("bevel planetary diff 80 16:23 x2 spiral -25", set(Differential, 80.0, 16, 23, 2, -25.0)),
            ("bevel planetary single 60 20:12 x3 spiral 35", set(Single, 60.0, 20, 12, 3, 35.0)),
            ("bevel planetary ring 45 20:10 x3", set(Ring, 45.0, 20, 10, 3, 0.0)),
            ("bevel planetary ring 60 24:12 x3 spiral 25", set(Ring, 60.0, 24, 12, 3, 25.0)),
            ("bevel planetary ring 30 20:10 x3 spiral -35", set(Ring, 30.0, 20, 10, 3, -35.0)),
        ] {
            assert!(plan(&s).is_ok(), "{label}: {:?}", plan(&s).err());
            variants.push((label.to_string(), s));
        }
    }
    // Differentials: every style with every drive, the rings moved onto their cases.
    {
        use crate::diff::{Diff, DriveKind, Mount, Style};
        for style in [Style::Open, Style::Biased, Style::TorsenB] {
            for drive in DriveKind::ALL {
                for mount in Mount::ALL {
                    let zp = if drive == DriveKind::Bevel { 13 } else { 11 };
                    let s = Spec {
                        diff: Diff {
                            drive,
                            mount,
                            drive_pinion_teeth: zp,
                            drive_spiral: if drive == DriveKind::Hypoid { 50.0 } else { 35.0 },
                            ..Diff::defaults_for(style)
                        },
                        ..Spec::defaults_for(Kind::Differential)
                    };
                    assert!(plan(&s).is_ok(), "{style:?} {drive:?} {mount:?}: {:?}", plan(&s).err());
                    variants.push((format!("differential {style:?} {drive:?} {mount:?}"), s));
                }
            }
        }
        let s = Spec { diff: Diff { spiral: 25.0, pinions: 4, ..Diff::defaults_for(Style::Open) }, ..Spec::defaults_for(Kind::Differential) };
        variants.push(("differential open spiral 25 x4".into(), s));
    }
    for (label, s) in &variants {
        check(s, label, &mut bad);
    }
    assert!(bad.is_empty(), "lofts the kernel would mis-align:\n{}", bad.join("\n"));
}

#[test]
fn a_pair_can_be_herringbone() {
    let s = Spec { helix: 20.0, herringbone: true, ..Spec::defaults_for(Kind::Pair) };
    let p = plan(&s).unwrap();
    // Each gear's teeth are two opposite halves: the helical body turns one way, then back.
    for b in &p.bodies {
        let twisted = b
            .steps
            .iter()
            .filter(|st| match st {
                Step::Loft { .. } => true,
                Step::Prism { twist, .. } => *twist != 0.0,
                _ => false,
            })
            .count();
        assert!(twisted >= 1, "{}: {:?}", b.name, b.steps.len());
    }
    let simple = crate::simple::outputs_of(&s);
    assert!(simple.herringbone);
    let back = crate::simple::solve(&simple, &s);
    assert!(back.spec.herringbone, "{:?}", back.notes);
    assert!(!back.notes.iter().any(|n| n.contains("not built")), "{:?}", back.notes);
}
