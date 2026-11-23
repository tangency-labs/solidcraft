//! Gear trains drawn as circles: reading, sizing, planning, meshing and the assembly.

use crate::V2;
use crate::mesh2d::{Verdict, check_mesh};
use crate::plan::{AssemblyLink, Kind, LinkKind, Plan, Spec};
use crate::simple::Simple;
use crate::sketch_rules::{Circle, Drawn, Line, Reading, TrainCircle, read, size_train, train_spec};

fn circle(x: f64, y: f64, r: f64) -> Circle {
    Circle { c: V2::new(x, y), r }
}

fn drawn(circles: Vec<Circle>) -> Drawn {
    Drawn { points: vec![], circles, lines: vec![] }
}

/// Reads, sizes and plans a drawing of circles as a train in `style`.
fn train(style: Kind, circles: Vec<Circle>, settings: Simple) -> Result<(Spec, Plan, Vec<String>), String> {
    let d = drawn(circles);
    let Reading::Train { circles: picked } = read(Some(style), &d)? else { panic!("not read as a train") };
    let cs: Vec<Circle> = picked.iter().map(|i| d.circles[*i]).collect();
    let (spec, notes) = train_spec(&cs, style, &settings, &Spec::defaults_for(style))?;
    let plan = crate::plan(&spec)?;
    Ok((spec, plan, notes))
}

/// Every mesh of the plan checks clean in 2D (no overlap, flanks in contact).
fn meshes_clean(p: &Plan, count: usize) {
    let r = check_mesh(p);
    assert_eq!(r.len(), count, "{:?}", p.meshes);
    for m in &r {
        assert_eq!(m.verdict, Verdict::Ok, "{}", m.message);
    }
}

fn fact(p: &Plan, label: &str) -> f64 {
    p.facts
        .iter()
        .find(|f| f.label == label)
        .unwrap_or_else(|| panic!("no fact {label}: {:?}", p.facts.iter().map(|f| &f.label).collect::<Vec<_>>()))
        .value
}

/// The assembly's ratios from gear 1 are the preview's speeds.
fn assembly_matches_preview(p: &Plan) {
    assert_eq!(p.assembly.members.len(), p.bodies.len());
    for (k, g) in p.preview.iter().enumerate() {
        let r = p.assembly.ratio(0, k);
        if g.speed != 0.0
            && k > 0
            && let Some(r) = r
        {
            assert!((r - g.speed).abs() < 1e-9, "gear {}: assembly {r}, preview {}", k + 1, g.speed);
        }
    }
}

/// Gear 1 Ø40, gear 2 Ø60 touching it, gear 3 Ø30 touching gear 2 above it.
fn three() -> Vec<Circle> {
    vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0), circle(50.0, 45.0, 15.0)]
}

#[test]
fn three_tangent_circles_make_a_three_gear_train() {
    let (spec, p, _) = train(Kind::Spur, three(), m25()).unwrap();
    let t = spec.train.as_ref().unwrap();
    // Module 2.5 gives 16, 24 and 12 teeth exactly: no shift needed, the free one clears undercut.
    assert!(t.gears.iter().all(|g| g.module == 2.5), "{:?}", t.gears);
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![16, 24, 12]);
    assert_eq!(t.links.len(), 2);
    assert_eq!(p.bodies.len(), 3);
    // Gear 3 at its drawn centre.
    assert!((p.bodies[2].frame.origin[0] - 50.0).abs() < 1e-9 && (p.bodies[2].frame.origin[1] - 45.0).abs() < 1e-9);
    meshes_clean(&p, 2);
    // Ratios: per mesh and from gear 1 (gear 3 turns the same way as gear 1, 16/12 as fast).
    assert!((fact(&p, "gears 1–2 ratio") - 1.5).abs() < 1e-12);
    assert!((fact(&p, "gears 2–3 ratio") - 0.5).abs() < 1e-12);
    assert!((fact(&p, "train ratio gear 1 → gear 3") - 0.75).abs() < 1e-12);
    assert!((fact(&p, "gears 2–3 centre distance") - 45.0).abs() < 1e-9);
    assert!(fact(&p, "gears 1–2 contact ratio") > 1.2);
    assert!((p.preview[2].speed - 16.0 / 12.0).abs() < 1e-12);
    // Assembly: three members, gear links along the train.
    assert_eq!(p.assembly.links.len(), 2);
    assert!(p.assembly.links.iter().all(|l| matches!(l, AssemblyLink::Gear { internal: false, .. })));
    assembly_matches_preview(&p);
    assert!((p.assembly.ratio(0, 2).unwrap() - 16.0 / 12.0).abs() < 1e-12);
}

#[test]
fn every_gear_meshes_at_any_angle_round_its_driver() {
    // Gear 3 at several angles round gear 2: the phasing follows the line of centres.
    for deg in [20.0f64, 75.0, 110.0, 160.0, 230.0, 300.0] {
        let a = deg.to_radians();
        let c = vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0), circle(50.0 + 45.0 * a.cos(), 45.0 * a.sin(), 15.0)];
        match train(Kind::Spur, c, Simple::default()) {
            Ok((_, p, _)) => meshes_clean(&p, 2),
            // Round towards gear 1, gear 3 runs into it (or its circle crosses): refused, not built.
            Err(e) => assert!(e.contains("gears 1 and 3"), "{deg}: {e}"),
        }
    }
}

#[test]
fn a_gear_inside_a_ring_with_an_idler() {
    // Ring Ø120 (gear 1), a Ø48 gear inside touching it, a Ø24 idler inside touching that one.
    let a = 120f64.to_radians();
    let c = vec![circle(0.0, 0.0, 60.0), circle(36.0, 0.0, 24.0), circle(36.0 + 36.0 * a.cos(), 36.0 * a.sin(), 12.0)];
    let (spec, p, _) = train(Kind::Spur, c, Simple { module: Some(3.0), ..Simple::default() }).unwrap();
    let t = spec.train.as_ref().unwrap();
    assert!(t.gears[0].ring && !t.gears[1].ring && !t.gears[2].ring);
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![40, 16, 8]);
    assert!(t.links.contains(&crate::plan::TrainLink { a: 1, b: 0, kind: LinkKind::Internal }));
    meshes_clean(&p, 2);
    assert!(p.meshes.iter().any(|m| m.internal && m.b == 0 && m.a == 1));
    // Inside the ring both turn the same way: gear 2 at 60/24, the idler the other way.
    assert!((p.preview[1].speed - 2.5).abs() < 1e-12);
    assert!((p.preview[2].speed + 5.0).abs() < 1e-12);
    assert!(p.assembly.links.iter().any(|l| matches!(l, AssemblyLink::Gear { a: 0, b: 1, internal: true, .. })));
    assembly_matches_preview(&p);
}

#[test]
fn a_gear_touching_nothing_is_an_error_naming_it() {
    let mut c = three();
    c.push(circle(130.0, 0.0, 10.0));
    let e = train(Kind::Spur, c.clone(), Simple::default()).unwrap_err();
    assert!(e.contains("gear 4 touches no other gear") && e.contains("from gear 2's"), "{e}");
    // Per member: only gear 4 is wrong; the others are sized and meshed.
    let tc: Vec<TrainCircle> = c.iter().map(|k| TrainCircle::of(*k)).collect();
    let sized = size_train(&tc, Kind::Spur, &m25(), &Spec::default());
    assert!(sized.spec.is_none());
    assert!(sized.errors[..3].iter().all(Option::is_none) && sized.errors[3].is_some(), "{:?}", sized.errors);
    assert_eq!(sized.members[1].meshes_with, vec![0, 2]);
    assert_eq!(sized.members[1].teeth, 24);
}

#[test]
fn a_circle_nearly_touching_is_refused_naming_it() {
    // Gear 3 one millimetre short of touching gear 2.
    let c = vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0), circle(50.0, 46.0, 15.0)];
    let e = train(Kind::Spur, c, Simple::default()).unwrap_err();
    assert!(e.contains("gear 3 touches no other gear") && e.contains("1 mm from gear 2's"), "{e}");
    // Crossing pitch circles on one layer.
    let c = vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0), circle(50.0, 40.0, 15.0)];
    let e = train(Kind::Spur, c, Simple::default()).unwrap_err();
    assert!(e.contains("pitch circles cross"), "{e}");
}

fn m25() -> Simple {
    Simple { module: Some(2.5), ..Simple::default() }
}

/// The plan's centre distance of the mesh of gears `a`–`b` (1-based) is the drawn one.
fn centre_kept(p: &Plan, a: usize, b: usize, drawn: f64) {
    let got = fact(p, &format!("gears {a}–{b} centre distance"));
    assert!((got - drawn).abs() < 1e-6, "gears {a}–{b}: {got} built, {drawn} drawn");
}

#[test]
fn a_three_gear_train_off_the_module_is_held_by_shift() {
    // Ø40, Ø61 and Ø30 touching in a row at module 2: 20, 31 (30.5 rounded) and 15 teeth; no
    // 0.5 % refusal, the drawn centre distances held by profile shift.
    let c = vec![circle(0.0, 0.0, 20.0), circle(50.5, 0.0, 30.5), circle(50.5, 45.5, 15.0)];
    let (spec, p, notes) = train(Kind::Spur, c, Simple { module: Some(2.0), ..Simple::default() }).unwrap();
    let t = spec.train.as_ref().unwrap();
    assert!(t.gears.iter().all(|g| g.module == 2.0), "{:?}", t.gears);
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![20, 31, 15]);
    assert!(notes.iter().any(|n| n.contains("gear 2: Ø61 drawn, Ø62 built")), "{notes:?}");
    centre_kept(&p, 1, 2, 50.5);
    centre_kept(&p, 2, 3, 45.5);
    meshes_clean(&p, 2);
}

#[test]
fn module_3_on_a_40_circle_is_13_teeth_and_a_shift() {
    // Ø40 is 13.33 teeth at module 3: 13 (Ø39), and Ø60 is 20. The 50 mm drawn centre distance
    // (49.5 standard) is held by a positive shift sum, most of it on the 13-tooth gear (undercut).
    let c = [circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0)];
    let tc: Vec<TrainCircle> = c.iter().map(|k| TrainCircle::of(*k)).collect();
    let sized = size_train(&tc, Kind::Spur, &Simple { module: Some(3.0), ..Simple::default() }, &Spec::defaults_for(Kind::Spur));
    assert!(sized.error.is_none(), "{:?}", sized.error);
    let spec = sized.spec.unwrap();
    let t = spec.train.as_ref().unwrap();
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![13, 20]);
    assert!(t.gears.iter().all(|g| g.module == 3.0));
    let x: Vec<f64> = t.gears.iter().map(|g| g.shift).collect();
    assert_eq!(sized.shifts, x);
    assert!(x[0] > 0.1 && x[0] > x[1], "{x:?}");
    // The shift sum the operating pressure angle gives for 50 mm.
    let inv = |a: f64| a.tan() - a;
    let a20 = 20f64.to_radians();
    let aw = (49.5 * a20.cos() / 50.0).acos();
    let sum = (inv(aw) - inv(a20)) * 33.0 / (2.0 * a20.tan());
    assert!((x[0] + x[1] - sum).abs() < 1e-9, "{x:?} vs {sum}");
    let p = crate::plan(&spec).unwrap();
    centre_kept(&p, 1, 2, 50.0);
    meshes_clean(&p, 1);
}

#[test]
fn the_automatic_module_gives_the_smallest_gear_17_teeth() {
    // Smallest Ø30: module 1.5 is the largest series I module giving it 17 or more (20).
    let (spec, p, _) = train(Kind::Spur, three(), Simple::default()).unwrap();
    let t = spec.train.as_ref().unwrap();
    assert!(t.gears.iter().all(|g| g.module == 1.5), "{:?}", t.gears);
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![27, 40, 20]);
    assert!(t.gears.iter().map(|g| g.teeth).min().unwrap() >= crate::sketch_rules::AUTO_TEETH);
    centre_kept(&p, 1, 2, 50.0);
    centre_kept(&p, 2, 3, 45.0);
    meshes_clean(&p, 2);
    // Tiny gears: no standard module gives 17 teeth; fewer with a note.
    let (m, z, note) = crate::sketch_rules::auto_module(&[(2.0, 1.0)], 8);
    assert_eq!((m, z), (0.25, vec![8]));
    assert!(note.unwrap().contains("profile shift"));
}

#[test]
fn size_by_teeth_takes_the_module_from_the_drawn_diameter() {
    // Gear 1 Ø41 with 16 teeth: module 41/16 (not standard). Gear 2 Ø60 takes 23 teeth.
    let mut tc: Vec<TrainCircle> = [circle(0.0, 0.0, 20.5), circle(50.5, 0.0, 30.0)].iter().map(|k| TrainCircle::of(*k)).collect();
    tc[0].teeth = Some(16);
    let sized = size_train(&tc, Kind::Spur, &Simple::default(), &Spec::defaults_for(Kind::Spur));
    assert!(sized.error.is_none(), "{:?}", sized.error);
    let spec = sized.spec.unwrap();
    let t = spec.train.as_ref().unwrap();
    assert!(t.gears.iter().all(|g| (g.module - 41.0 / 16.0).abs() < 1e-12), "{:?}", t.gears);
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![16, 23]);
    assert!((sized.members[0].pitch_diameter - 41.0).abs() < 1e-9);
    let p = crate::plan(&spec).unwrap();
    centre_kept(&p, 1, 2, 50.5);
    meshes_clean(&p, 1);
}

#[test]
fn shifts_past_the_limits_are_refused_naming_sizes_that_work() {
    // Two Ø20 touching at module 3: 7 teeth each (Ø21), 20 mm apart: the shift sum is negative,
    // and neither 7-tooth gear may be cut deeper.
    let c = [circle(0.0, 0.0, 10.0), circle(20.0, 0.0, 10.0)];
    let tc: Vec<TrainCircle> = c.iter().map(|k| TrainCircle::of(*k)).collect();
    let sized = size_train(&tc, Kind::Spur, &Simple { module: Some(3.0), ..Simple::default() }, &Spec::defaults_for(Kind::Spur));
    assert!(sized.spec.is_none() && sized.errors.iter().all(Option::is_some), "{:?}", sized.errors);
    let e = sized.error.unwrap();
    assert!(e.contains("gears 1 and 2") && e.contains("undercut") && e.contains("at module 3 (gear 1 z7 and gear 2 z7)"), "{e}");
    assert!(e.contains("module 2.5 (gear 1 z8, gear 2 z8)") && e.contains("with 6 teeth (or draw it Ø18)"), "{e}");
}

#[test]
fn a_module_block_sizes_all_its_gears() {
    // Gears 1 and 2 in a block at module 2 (each carries the block's module), gear 3 free: the
    // whole meshing group takes module 2: 21, 29 and 15 teeth, the drawn centres held.
    let c = [circle(0.0, 0.0, 20.6), circle(50.0, 0.0, 29.4), circle(50.0, 44.4, 15.0)];
    let mut tc: Vec<TrainCircle> = c.iter().map(|k| TrainCircle::of(*k)).collect();
    tc[0].module = Some(2.0);
    tc[1].module = Some(2.0);
    let sized = size_train(&tc, Kind::Spur, &Simple::default(), &Spec::defaults_for(Kind::Spur));
    assert!(sized.error.is_none(), "{:?}", sized.error);
    assert!(sized.members.iter().all(|m| m.module == 2.0));
    assert_eq!(sized.members.iter().map(|m| m.teeth).collect::<Vec<_>>(), vec![21, 29, 15]);
    let p = crate::plan(&sized.spec.unwrap()).unwrap();
    centre_kept(&p, 1, 2, 50.0);
    centre_kept(&p, 2, 3, 44.4);
    meshes_clean(&p, 2);
    // A gear asking another module than its block's meshing partners: refused.
    tc[2].module = Some(2.5);
    let sized = size_train(&tc, Kind::Spur, &Simple::default(), &Spec::defaults_for(Kind::Spur));
    assert!(sized.errors[2].as_deref().is_some_and(|e| e.contains("meshing gears share one module")), "{:?}", sized.errors);
}

#[test]
fn stages_follow_the_rule_where_they_can() {
    use crate::sketch_rules::{StageGear, size_stage};
    let base = Spec::defaults_for(Kind::Spur);
    // Rack: Ø40 at module 3, 13 teeth; the pitch line at 20 mm by the shift (40 − 39) ÷ 6.
    let s = size_stage(Kind::Rack, &[StageGear::of(40.0)], Some(3.0), &base).unwrap();
    assert_eq!(s.teeth, vec![13]);
    assert!((s.shift[0] - 1.0 / 6.0).abs() < 1e-12);
    // Worm: wheel Ø61.4 at module 2 (31 teeth, Ø62), the worm's Ø20 kept: the wheel's shift.
    let s = size_stage(Kind::Worm, &[StageGear::of(61.4), StageGear::of(20.0)], Some(2.0), &base).unwrap();
    assert_eq!(s.teeth, vec![31, base.starts.max(1)]);
    assert!((s.shift[0] + 0.15).abs() < 1e-9 && s.shift[1] == 0.0, "{:?}", s.shift);
    // Crossed helical at 45° each: 14 and 21 teeth at module 2; the shift sum makes up the
    // centre distance: (100 − 99) ÷ 2 ÷ 2.
    let f = 1.0 / 45f64.to_radians().cos();
    let g = |d: f64| StageGear { d, factor: f, teeth: None };
    let s = size_stage(Kind::CrossedHelical, &[g(40.0), g(60.0)], Some(2.0), &base).unwrap();
    assert_eq!(s.teeth, vec![14, 21]);
    let built = 2.0 * f * 35.0;
    assert!((s.shift[0] + s.shift[1] - (100.0 - built) / 4.0).abs() < 1e-9, "{:?}", s.shift);
    // Bevel: gear 1's outer Ø40 kept (13 teeth, module 40/13); gear 2 Ø62 → 20 teeth, the ratio
    // noted; balanced shifts favour the 13-tooth gear.
    let bevel = Spec { shaft_angle: 90.0, ..Spec::defaults_for(Kind::Bevel) };
    let s = size_stage(Kind::Bevel, &[StageGear::of(40.0), StageGear::of(62.0)], Some(3.0), &bevel).unwrap();
    assert_eq!(s.teeth, vec![13, 20]);
    assert!((s.module - 40.0 / 13.0).abs() < 1e-12);
    assert!((s.pitch_diameter[0] - 40.0).abs() < 1e-12);
    assert!(s.shift[0] >= 0.0 && (s.shift[0] + s.shift[1]).abs() < 1e-12, "{:?}", s.shift);
    assert!(s.notes.iter().any(|n| n.contains("ratio")), "{:?}", s.notes);
    // A pair past the limits: refused with sizes that work.
    let e = size_stage(Kind::Pair, &[StageGear::of(20.0), StageGear::of(20.0)], Some(3.0), &base).unwrap_err();
    assert!(e.contains("undercut") && e.contains("module 2.5 (gear 1 z8, gear 2 z8)"), "{e}");
}

#[test]
fn drawn_centres_are_kept_by_profile_shift() {
    // 0.3 % off standard sizes: module 2.5 teeth, the centre distances reached by shifting.
    let c = vec![circle(0.0, 0.0, 20.06), circle(50.12, 0.0, 30.06), circle(50.12, 45.1, 15.04)];
    let (spec, p, notes) = train(Kind::Spur, c, m25()).unwrap();
    let t = spec.train.as_ref().unwrap();
    assert_eq!(t.gears.iter().map(|g| g.teeth).collect::<Vec<_>>(), vec![16, 24, 12]);
    assert!(t.gears[1].shift.abs() > 1e-6, "{:?}", t.gears);
    assert!(notes.iter().any(|n| n.contains("drawn, Ø")), "{notes:?}");
    assert!((fact(&p, "gears 1–2 centre distance") - 50.12).abs() < 1e-6);
    assert!((fact(&p, "gears 2–3 centre distance") - 45.1).abs() < 1e-6);
    meshes_clean(&p, 2);
}

#[test]
fn helical_hands_alternate_across_outside_meshes() {
    let (spec, p, _) = train(Kind::Helical, three(), Simple { helix: Some(15.0), ..Simple::default() }).unwrap();
    let h: Vec<f64> = spec.train.as_ref().unwrap().gears.iter().map(|g| g.helix).collect();
    assert_eq!(h, vec![15.0, -15.0, 15.0]);
    meshes_clean(&p, 2);
    // The bodies twist the matching ways (gear 1 and gear 3 alike, gear 2 the other way).
    let twist = |b: usize| match &p.bodies[b].steps[0] {
        crate::plan::Step::Prism { twist, .. } => *twist,
        s => panic!("{s:?}"),
    };
    assert!(twist(0) > 0.0 && twist(1) < 0.0 && twist(2) > 0.0);
    // Inside a ring the hands are the same.
    let a = 120f64.to_radians();
    let c = vec![circle(0.0, 0.0, 60.0), circle(36.0, 0.0, 24.0), circle(36.0 + 36.0 * a.cos(), 36.0 * a.sin(), 12.0)];
    let (spec, p, _) = train(Kind::Helical, c, Simple { helix: Some(15.0), ..Simple::default() }).unwrap();
    let h: Vec<f64> = spec.train.as_ref().unwrap().gears.iter().map(|g| g.helix).collect();
    assert_eq!(h, vec![15.0, 15.0, -15.0]);
    meshes_clean(&p, 2);
}

#[test]
fn concentric_circles_are_compound_gears_on_one_shaft() {
    // Gear 2 Ø60 and gear 3 Ø30 on one shaft; gear 4 Ø40 meshes with gear 3 (crossing gear 2's
    // circle, a layer away).
    let c = vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0), circle(50.0, 0.0, 15.0), circle(50.0, 35.0, 20.0)];
    let (spec, p, _) = train(Kind::Spur, c, Simple::default()).unwrap();
    let t = spec.train.as_ref().unwrap();
    assert!(t.links.iter().any(|l| l.kind == LinkKind::Compound && l.a == 1 && l.b == 2));
    assert!(t.gears[2].z > t.gears[1].z && t.gears[3].z == t.gears[2].z && t.gears[0].z == 0.0);
    meshes_clean(&p, 2);
    assert!((p.preview[2].speed - p.preview[1].speed).abs() < 1e-12);
    let (z1, z2, z3, z4) = (t.gears[0].teeth, t.gears[1].teeth, t.gears[2].teeth, t.gears[3].teeth);
    let want = (f64::from(z2) / f64::from(z1)) * (f64::from(z4) / f64::from(z3));
    assert!((fact(&p, "train ratio gear 1 → gear 4") - want).abs() < 1e-12);
    // Gears 2 and 3 are one body (one part on their shaft).
    assert_eq!(p.bodies.len(), 3);
    assert_eq!((p.members[1].body, p.members[2].body), (Some(1), Some(1)));
}

#[test]
fn three_gears_round_a_loop_lock() {
    let h = 40.0 * (60f64.to_radians()).sin();
    let c = vec![circle(0.0, 0.0, 20.0), circle(40.0, 0.0, 20.0), circle(20.0, h, 20.0)];
    let e = train(Kind::Spur, c, Simple::default()).unwrap_err();
    assert!(e.contains("locks"), "{e}");
}

#[test]
fn a_ring_meshing_outside_is_refused() {
    // Gear 2 inside ring 1, gear 3 touching the ring's outside.
    let c = vec![circle(0.0, 0.0, 60.0), circle(36.0, 0.0, 24.0), circle(-80.0, 0.0, 20.0)];
    let e = train(Kind::Spur, c, Simple::default()).unwrap_err();
    assert!(e.contains("gear 1 is a ring gear") && e.contains("gear 3 touches its outside"), "{e}");
}

#[test]
fn pairs_and_planetary_sets_keep_their_own_planners() {
    let d = drawn(vec![circle(0.0, 0.0, 20.0), circle(50.0, 0.0, 30.0)]);
    assert!(matches!(read(Some(Kind::Helical), &d), Ok(Reading::Mode { kind: Kind::Pair, .. })));
    let d = drawn(vec![circle(0.0, 0.0, 18.0), circle(36.0, 0.0, 18.0), circle(0.0, 0.0, 54.0)]);
    assert!(matches!(read(Some(Kind::Spur), &d), Ok(Reading::Mode { kind: Kind::Planetary, .. })));
    assert!(matches!(read(None, &d), Ok(Reading::Mode { kind: Kind::Planetary, .. })));
    // A family named is measured by its own rule, as before.
    let d = drawn(vec![circle(0.0, 0.0, 60.0), circle(40.0, 0.0, 20.0)]);
    assert!(matches!(read(Some(Kind::Internal), &d), Ok(Reading::Mode { kind: Kind::Internal, .. })));
    // With a tooth style, a gear inside a ring is a train: both are built.
    let Ok(Reading::Train { .. }) = read(Some(Kind::Spur), &d) else { panic!() };
    let (_, p, _) = train(Kind::Spur, vec![circle(0.0, 0.0, 60.0), circle(40.0, 0.0, 20.0)], Simple::default()).unwrap();
    assert_eq!(p.bodies.len(), 2);
    meshes_clean(&p, 1);
    // The gear first and the ring second: the ring turned to mesh with it.
    let (spec, p, _) = train(Kind::Spur, vec![circle(40.0, 0.0, 20.0), circle(0.0, 0.0, 60.0)], Simple::default()).unwrap();
    assert!(spec.train.as_ref().unwrap().gears[1].ring);
    meshes_clean(&p, 1);
    assert!((p.preview[1].speed - 1.0 / 3.0).abs() < 1e-12);
    // One circle: a single gear of the style.
    let d = drawn(vec![circle(0.0, 0.0, 20.0)]);
    assert!(matches!(read(Some(Kind::Herringbone), &d), Ok(Reading::Mode { kind: Kind::Herringbone, .. })));
}

#[test]
fn lines_read_as_the_set_they_draw_in_any_style() {
    let line = |ax: f64, ay: f64, bx: f64, by: f64| Line { a: V2::new(ax, ay), b: V2::new(bx, by) };
    let one = |l: Line| Drawn { points: vec![], circles: vec![circle(0.0, 0.0, 30.0)], lines: vec![l] };
    let kind = |style: Option<Kind>, d: &Drawn| match read(style, d) {
        Ok(Reading::Mode { kind, .. }) => kind,
        r => panic!("{r:?}"),
    };
    assert_eq!(kind(Some(Kind::Spur), &one(line(-40.0, 30.0, 40.0, 30.0))), Kind::Rack);
    assert_eq!(kind(Some(Kind::Worm), &one(line(-40.0, 30.0, 40.0, 30.0))), Kind::Worm);
    assert_eq!(kind(None, &one(line(-40.0, 30.0, 40.0, 30.0))), Kind::Worm);
    assert_eq!(kind(Some(Kind::Spur), &one(line(-40.0, 0.0, 40.0, 0.0))), Kind::FaceGear);
    assert_eq!(kind(Some(Kind::Spur), &one(line(-40.0, 10.0, 40.0, 10.0))), Kind::Hypoid);
    assert_eq!(kind(Some(Kind::Helical), &one(line(-40.0, 45.0, 40.0, 45.0))), Kind::CrossedHelical);
    let side = Drawn { points: vec![V2::ZERO], circles: vec![], lines: vec![line(0.0, 0.0, 0.0, -50.0), line(0.0, 0.0, 50.0, 0.0)] };
    assert_eq!(kind(Some(Kind::Spur), &side), Kind::Bevel);
    assert_eq!(kind(Some(Kind::Helical), &side), Kind::SpiralBevel);
    let Ok(Reading::Mode { measured, .. }) = read(Some(Kind::Spur), &side) else { panic!() };
    assert!((measured.shaft_angle.unwrap() - 90.0).abs() < 1e-9);
    // Circles and lines together: not yet.
    let mixed = Drawn { points: vec![], circles: three(), lines: vec![line(-40.0, 20.0, 40.0, 20.0)] };
    assert!(read(Some(Kind::Spur), &mixed).unwrap_err().contains("not made yet"));
}

#[test]
fn a_train_placed_by_its_sketch_turns_with_it() {
    let (mut spec, _, _) = train(Kind::Spur, three(), Simple::default()).unwrap();
    // Sketch on world XZ (normal −Y... here: its x along world X, its normal along world Y).
    let frame = crate::plan::Frame { origin: [10.0, 0.0, 0.0], x: [1.0, 0.0, 0.0], z: [0.0, 1.0, 0.0] };
    spec.placement = Some(crate::sketch_rules::train_placement(V2::ZERO, &frame, [0.0; 3]));
    let p = crate::plan(&spec).unwrap();
    // Gear 3 at sketch (50, 45): the frame's y is z × x = (0, 0, -1).
    let o = p.bodies[2].frame.origin;
    assert!((o[0] - 60.0).abs() < 1e-9 && o[1].abs() < 1e-9 && (o[2] + 45.0).abs() < 1e-9, "{o:?}");
    assert!((p.bodies[2].frame.z[1] - 1.0).abs() < 1e-12);
    assembly_matches_preview(&p);
}
