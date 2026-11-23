//! Differentials: every style and drive plans, every mesh is proved with the 3D checker, the
//! assembled members clear each other and the case, and Simple mode designs them by outputs. Run
//! with `--nocapture` for the numbers.

use crate::diff::{Diff, DriveKind, Goal, Mount, Style, solve, torsen_b_bias};
use crate::mesh2d::Verdict;
use crate::mesh3d::{MeshPair3, Pair, Solid, overlap};
use crate::plan::{AssemblyLink, Kind, Plan, Spec, plan};

fn spec(d: Diff) -> Spec {
    Spec { diff: d, ..Spec::defaults_for(Kind::Differential) }
}

fn fact(p: &Plan, label: &str) -> f64 {
    p.facts
        .iter()
        .find(|f| f.label == label)
        .unwrap_or_else(|| panic!("no fact {label}: {:?}", p.facts.iter().map(|f| f.label.clone()).collect::<Vec<_>>()))
        .value
}

fn show(label: &str, p: &Plan) {
    eprintln!("== {label}: {} bodies: {}", p.bodies.len(), p.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>().join(", "));
    for l in [
        "case outside diameter",
        "case length (journal ends)",
        "ring gear pitch diameter",
        "ring gear least root diameter",
        "ring bolts",
        "ring bolt size (M)",
        "drive pinion to case clearance",
        "torque bias ratio TBR (estimate)",
        "pinion axis angle to the axle",
    ] {
        if let Some(f) = p.facts.iter().find(|f| f.label == l) {
            eprint!("   {l} = {:.3};", f.value);
        }
    }
    eprintln!();
    for w in &p.warnings {
        eprintln!("   ! {w}");
    }
}

fn styles() -> Vec<(String, Spec)> {
    let mut v = Vec::new();
    for style in [Style::Open, Style::Biased, Style::TorsenB, Style::TorsenA] {
        for drive in DriveKind::ALL {
            for mount in Mount::ALL {
                if drive == DriveKind::None && mount == Mount::Integral {
                    continue;
                }
                // (The straight bevel planner's 11-tooth pinion interferes with a 41 ring on its own: 13.)
                let zp = if drive == DriveKind::Bevel { 13 } else { 11 };
                v.push((
                    format!("{style:?} {drive:?} {mount:?}"),
                    spec(Diff {
                        drive,
                        mount,
                        keep_out: true,
                        drive_pinion_teeth: zp,
                        drive_spiral: if drive == DriveKind::Hypoid { 50.0 } else { 35.0 },
                        ..Diff::defaults_for(style)
                    }),
                ));
            }
        }
    }
    v
}

#[test]
fn every_style_and_drive_plans() {
    for (label, s) in styles() {
        let t = std::time::Instant::now();
        let p = plan(&s).unwrap_or_else(|e| panic!("{label}: {e}"));
        show(&format!("{label} ({:.2} s)", t.elapsed().as_secs_f64()), &p);
        assert!(p.clearance.is_some(), "{label}: no keep-out body");
        assert!(!p.meshes3d.is_empty(), "{label}");
        assert_eq!(p.assembly.members.len(), p.bodies.len(), "{label}");
        // The links form a tree: no member driven twice.
        let mut driven = vec![0; p.bodies.len()];
        for l in &p.assembly.links {
            driven[l.members().1] += 1;
        }
        assert!(driven.iter().all(|n| *n <= 1), "{label}: {:?}", p.assembly.links);
        // Carried members ride on the case.
        assert!(p.assembly.members.iter().any(|m| m.carrier == Some(0)), "{label}");
    }
}

/// Every mesh of every style through two tooth pitches: no flank inside the other deeper than the
/// checker's noise, flanks in contact (or the backlash apart); half a pitch out they collide.
#[test]
fn every_mesh_is_proved() {
    const STEPS: usize = 12;
    for (label, s) in [
        ("open", spec(Diff { drive: DriveKind::None, ..Diff::defaults_for(Style::Open) })),
        ("open 4 pinions", spec(Diff { drive: DriveKind::None, pinions: 4, ..Diff::defaults_for(Style::Open) })),
        ("biased 12:18", spec(Diff { drive: DriveKind::None, ..Diff::defaults_for(Style::Biased) })),
        ("torsen B", spec(Diff { drive: DriveKind::None, ..Diff::defaults_for(Style::TorsenB) })),
        ("torsen B backlash", spec(Diff { drive: DriveKind::None, backlash: 0.08, ..Diff::defaults_for(Style::TorsenB) })),
        ("torsen A", spec(Diff { drive: DriveKind::None, ..Diff::defaults_for(Style::TorsenA) })),
    ] {
        let p = plan(&s).unwrap_or_else(|e| panic!("{label}: {e}"));
        let m = s.diff.module;
        for mp in &p.meshes3d {
            let MeshPair3 { bodies: Some([a, b]), ratio, period, gap, labels, .. } = mp else { panic!("{label}: mesh without bodies") };
            let t = std::time::Instant::now();
            let mut pair = Pair::for_run(a, b, 0.03 * m, 0.1 * m, *ratio, -period, 2.0 * period);
            let r = pair.run(*ratio, 0.0, 0.0, 2.0 * period, STEPS);
            let half = 0.5 * period * ratio.abs();
            let off = pair.at(0.0, half);
            let names = labels.clone().unwrap_or_default().join(" / ");
            let line = format!(
                "{label:<18} {names:<28} pen {:>8.5} m  gap {:>8.5}..{:>8.5} m (wants {:.4}..{:.4} mm)  off: pen {:.3} mm  {:.1} s",
                r.penetration / m,
                r.gap_min / m,
                r.gap_max / m,
                gap.0,
                gap.1,
                off.penetration,
                t.elapsed().as_secs_f64()
            );
            eprintln!("{line}");
            assert!(r.penetration <= 2.0e-3 * m, "{line}: flanks cross");
            assert!(r.gap_max <= gap.1 + 2.5e-3 * m, "{line}: lose contact");
            assert!(off.penetration > 0.1 * m, "{line}: half a pitch out should collide");
        }
    }
}

/// The drives mesh as the window checks them (the coarse 3D check), moved onto the case.
#[test]
fn every_drive_meshes() {
    for (label, s) in styles() {
        if s.diff.drive == DriveKind::None || s.diff.style != Style::Open {
            continue;
        }
        let p = plan(&s).unwrap();
        let mp = p.meshes3d.last().unwrap();
        let r = crate::mesh3d::check(&p, mp);
        eprintln!("{label}: {}", r.message);
        // The same ring and pinion from their own planner, as they come.
        if let Some(sub) = crate::diff::drive_spec_for_tests(&s) {
            let plain = plan(&Spec { bore2: Default::default(), ..sub }).unwrap();
            if let Some(m) = plain.meshes3d.first() {
                eprintln!("   on its own: {}", crate::mesh3d::check(&plain, m).message);
            }
        }
        assert_eq!(r.verdict, Verdict::Ok, "{label}: {}", r.message);
    }
}

/// The whole assembly: every pair of members that don't mesh (and aren't fixed together) clears
/// the other at several turns of each about its own axis, the case included.
#[test]
fn the_assembly_has_no_collisions() {
    // Every style with no drive, a separate spiral bevel ring and an integral helical one (the case
    // is the same with the other drives; their meshes are checked by every_drive_meshes).
    let picked = |label: &String| {
        label.ends_with("None Bolted")
            || label.ends_with("SpiralBevel Bolted")
            || label.ends_with("Helical Integral")
            || label == "Open Hypoid Bolted"
    };
    for (label, s) in styles().into_iter().filter(|(l, _)| picked(l)) {
        let p = plan(&s).unwrap();
        let m = s.diff.module;
        let names: Vec<&str> = p.bodies.iter().map(|b| b.name.as_str()).collect();
        let meshing = |a: &str, b: &str| {
            p.meshes3d.iter().any(|mp| mp.bodies.as_ref().is_some_and(|[x, y]| (x.name == a && y.name == b) || (x.name == b && y.name == a)))
        };
        let rigid = |i: usize, j: usize| {
            p.assembly.links.iter().any(|l| matches!(l, AssemblyLink::Rigid { .. }) && (l.members() == (i, j) || l.members() == (j, i)))
        };
        let solids: Vec<Solid> = p.bodies.iter().map(|b| Solid::new(b, 0.05 * m)).collect();
        let t = std::time::Instant::now();
        let mut worst = (0.0f64, String::new());
        for i in 0..solids.len() {
            for j in i + 1..solids.len() {
                if meshing(names[i], names[j]) || rigid(i, j) {
                    continue;
                }
                for turn in [0.0, 0.9] {
                    let (mut a, mut b) = (solids[i].clone(), solids[j].clone());
                    // The case and what is fixed to it stay; the rest turn on their axes.
                    if i > 0 && !rigid(0, i) {
                        a.spin = turn * (1.0 + 0.61 * i as f64);
                    }
                    if !rigid(0, j) {
                        b.spin = turn * (1.0 + 0.61 * j as f64);
                    }
                    let v = overlap(&a, &b, 0.2 * m);
                    if v > worst.0 {
                        worst = (v, format!("{} / {} at {turn}", names[i], names[j]));
                    }
                    assert!(v < 0.01 * m * m * m, "{label}: {} and {} overlap by {v:.3} mm³ (turn {turn})", names[i], names[j]);
                }
            }
        }
        eprintln!("{label}: worst {:.4} mm³ ({}), {:.1} s", worst.0, worst.1, t.elapsed().as_secs_f64());
    }
}

#[test]
fn torque_split_and_speeds() {
    let p = plan(&spec(Diff { drive: DriveKind::None, ..Diff::defaults_for(Style::Biased) })).unwrap();
    assert!((fact(&p, "torque split ratio T_B / T_A (side B over side A)") - 1.5).abs() < 1e-12);
    assert!((fact(&p, "torque share side A") - 0.4).abs() < 1e-12);
    // 12 : 18 with 12-tooth pinions: cos Σ = 6 / 24.
    assert!((fact(&p, "pinion axis angle to the axle") - 0.25f64.acos().to_degrees()).abs() < 1e-6);
    assert!((fact(&p, "side B speed with side A held (× case speed)") - 30.0 / 18.0).abs() < 1e-12);
    let open = plan(&spec(Diff::defaults_for(Style::Open))).unwrap();
    assert!((fact(&open, "torque share side A") - 0.5).abs() < 1e-12);
    assert!((fact(&open, "final drive ratio") - 41.0 / 11.0).abs() < 1e-12);
    // The assembly turns as the gears do: case per drive pinion turn, side B per side A turn
    // relative to the case.
    let a = &open.assembly;
    let r = a.ratio(2, 0).unwrap();
    assert!((r.abs() - 11.0 / 41.0).abs() < 1e-9, "{r}");
    let (sa, sb) = (
        open.bodies.iter().position(|b| b.name.starts_with("Side gear A")).unwrap(),
        open.bodies.iter().position(|b| b.name.starts_with("Side gear B")).unwrap(),
    );
    let r = a.ratio(sa, sb).unwrap();
    assert!((r.abs() - 1.0).abs() < 1e-9, "{r}");
}

#[test]
fn torsen_bias_estimate_is_sane() {
    let (t, fw, fs) = torsen_b_bias(18, 11, 2.0, 30.0, 20.0, 0.1, 10.0);
    eprintln!("TBR {t:.3}, pocket {fw:.3}, thrust {fs:.3}");
    assert!(t > 1.5 && t < 4.0, "{t}");
    // More friction, more bias; none, almost none.
    assert!(torsen_b_bias(18, 11, 2.0, 30.0, 20.0, 0.15, 10.0).0 > t);
    assert!(torsen_b_bias(18, 11, 2.0, 30.0, 20.0, 0.0, 10.0).0 < 1.05);
    let p = plan(&spec(Diff { drive: DriveKind::None, ..Diff::defaults_for(Style::TorsenB) })).unwrap();
    assert!(fact(&p, "torque bias ratio TBR (estimate)") > 1.5);
}

#[test]
fn bad_designs_are_refused() {
    // Equal side gears in the biased style.
    let e = plan(&spec(Diff { side_teeth2: 0, ..Diff::defaults_for(Style::Biased) })).unwrap_err();
    assert!(e.contains("different tooth count"), "{e}");
    // An axle too thick for the side gears.
    let e = plan(&spec(Diff { axle_diameter: 40.0, ..Diff::default() })).unwrap_err();
    assert!(e.contains("axle") || e.contains("bore"), "{e}");
    // A ring gear too small to go round the case.
    let e = plan(&spec(Diff { drive_module: 1.0, ..Diff::default() })).unwrap_err();
    assert!(e.contains("ring") || e.contains("case"), "{e}");
    // Too many Torsen pairs.
    let e = plan(&spec(Diff { pinions: 8, drive: DriveKind::None, ..Diff::defaults_for(Style::TorsenB) })).unwrap_err();
    assert!(e.contains("don't fit"), "{e}");
    // Three Torsen A pairs of these elements crowd each other.
    let e = plan(&spec(Diff { pinions: 3, drive: DriveKind::None, ..Diff::defaults_for(Style::TorsenA) })).unwrap_err();
    assert!(e.contains("don't fit"), "{e}");
    // A Torsen A whose side gears aren't worms.
    let e = plan(&spec(Diff { helix: 20.0, ..Diff::defaults_for(Style::TorsenA) })).unwrap_err();
    assert!(e.contains("Torsen Type A"), "{e}");
}

#[test]
fn simple_mode_designs_by_outputs() {
    let base = Spec::defaults_for(Kind::Differential);
    for goal in [
        Goal { axle_diameter: 20.0, ratio: 3.73, torque: 60.0, ..Goal::default() },
        Goal { style: Style::Biased, axle_diameter: 16.0, ratio: 4.1, bias: 1.5, ..Goal::default() },
        Goal { style: Style::TorsenB, drive: DriveKind::Helical, axle_diameter: 16.0, ratio: 3.5, bias: 2.5, ..Goal::default() },
        Goal { drive: DriveKind::Hypoid, axle_diameter: 18.0, ratio: 3.9, ..Goal::default() },
    ] {
        let t = std::time::Instant::now();
        let sol = solve(&goal, &base).unwrap_or_else(|e| panic!("{goal:?}: {e}"));
        eprintln!("{goal:?}\n   -> {} ({:.1} s)", sol.notes.join("; "), t.elapsed().as_secs_f64());
        let p = plan(&sol.spec).unwrap();
        let ratio = fact(&p, "final drive ratio");
        assert!((ratio - goal.ratio).abs() < 0.15, "{ratio}");
        if goal.torque > 0.0 {
            let sf = p.facts.iter().filter(|f| f.label.ends_with("bending safety SF")).map(|f| f.value).fold(f64::INFINITY, f64::min);
            assert!(sf >= crate::strength::SF_MIN, "{sf}");
        }
        assert_eq!(crate::diff::goal_of(&sol.spec).style, goal.style);
    }
}

/// Each part on its own builds just its bodies, the design staying the whole differential's;
/// the meshes and the assembly are those of what is built.
#[test]
fn each_part_alone_builds_its_bodies() {
    use crate::diff::Parts;
    let none =
        Parts { side_a: false, side_b: false, pinions: false, which_pinions: vec![], pins: false, case: false, ring: false, drive_pinion: false };
    let full = plan(&spec(Diff::defaults_for(Style::Open))).unwrap();
    let names = |p: &Plan| p.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    for (label, parts, want) in [
        ("case", Parts { case: true, ..none.clone() }, vec!["Differential case"]),
        ("ring", Parts { ring: true, ..none.clone() }, vec!["Ring gear z41"]),
        ("drive pinion", Parts { drive_pinion: true, ..none.clone() }, vec!["Drive pinion z11"]),
        ("side A", Parts { side_a: true, ..none.clone() }, vec!["Side gear A z16"]),
        ("side B", Parts { side_b: true, ..none.clone() }, vec!["Side gear B z16"]),
        ("pinions", Parts { pinions: true, ..none.clone() }, vec!["Pinion 1 z10", "Pinion 2 z10"]),
        ("pinion 2", Parts { pinions: true, which_pinions: vec![2], ..none.clone() }, vec!["Pinion 2 z10"]),
        ("pins", Parts { pins: true, ..none.clone() }, vec!["Pinion pins"]),
        ("gears only", Parts::gears_only(), vec!["Side gear A z16", "Side gear B z16", "Pinion 1 z10", "Pinion 2 z10"]),
        ("ring and drive pinion", Parts { ring: true, drive_pinion: true, ..none.clone() }, vec!["Ring gear z41", "Drive pinion z11"]),
    ] {
        let p = plan(&spec(Diff { parts, ..Diff::defaults_for(Style::Open) })).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(names(&p), want, "{label}");
        // Designed as the whole differential.
        for f in ["final drive ratio", "case outside diameter", "pinion pin diameter"] {
            assert_eq!(fact(&p, f), fact(&full, f), "{label}: {f}");
        }
        // The assembly is the bodies built; its links only join built members, as a tree.
        assert_eq!(p.assembly.members.len(), p.bodies.len(), "{label}");
        let mut driven = vec![0; p.bodies.len()];
        for l in &p.assembly.links {
            let (a, b) = l.members();
            assert!(a < p.bodies.len() && b < p.bodies.len(), "{label}: {l:?}");
            driven[b] += 1;
        }
        assert!(driven.iter().all(|n| *n <= 1), "{label}");
        // What is built still meshes.
        for mp in &p.meshes3d {
            let r = crate::mesh3d::check(&p, mp);
            assert_eq!(r.verdict, Verdict::Ok, "{label}: {}", r.message);
        }
    }
    // The gear set alone: its four meshes, side A driving the pinions and side B by pinion 1,
    // the pinions on fixed axes (no case built).
    let g = plan(&spec(Diff { parts: Parts::gears_only(), ..Diff::defaults_for(Style::Open) })).unwrap();
    assert_eq!(g.meshes3d.len(), 4);
    assert_eq!(g.assembly.links.len(), 3);
    assert!(g.assembly.members.iter().all(|m| m.carrier.is_none()));
    assert!(g.clearance.is_none());
    // Torsen B: elements by number (1a and 1b), with the sides they mesh.
    let t = plan(&spec(Diff {
        drive: DriveKind::None,
        parts: Parts { which_pinions: vec![1, 2], ..Parts::gears_only() },
        ..Diff::defaults_for(Style::TorsenB)
    }))
    .unwrap();
    assert_eq!(names(&t), vec!["Side gear A z18", "Side gear B z18", "Element 1a z11", "Element 1b z11"]);
    assert_eq!(t.meshes3d.len(), 3);
    // An integral ring without its case is built as a separate ring.
    let r = plan(&spec(Diff { mount: Mount::Integral, parts: Parts { ring: true, ..none.clone() }, ..Diff::defaults_for(Style::Open) })).unwrap();
    assert_eq!(names(&r), vec!["Ring gear z41"]);
    // Nothing at all is refused; the keep-out body comes whatever is built.
    assert!(plan(&spec(Diff { parts: none.clone(), ..Diff::defaults_for(Style::Open) })).is_err());
    let k = plan(&spec(Diff { keep_out: true, parts: Parts { case: true, ..none }, ..Diff::defaults_for(Style::Open) })).unwrap();
    assert!(k.clearance.is_some());
}
