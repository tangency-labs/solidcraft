//! Drivetrain analysis: ratios and paths between members, the drive table, stages, mesh checks.

use std::f64::consts::TAU;

use crate::V2;
use crate::analysis::*;
use crate::axes::Axis;
use crate::plan::{Assembly, AssemblyLink, AssemblyMember, Frame, Kind, Plan, Spec, plan};
use crate::simple::Simple;
use crate::sketch_rules::{Circle, Drawn, Reading, read, train_spec};

use crate::test_util::close_rel as close;

fn circle(x: f64, y: f64, r: f64) -> Circle {
    Circle { c: V2::new(x, y), r }
}

/// system_tests' ten gears (module 2, radius = teeth): 1→2; 2=3 on a shaft; 3→4→5; 5=6; 6→7→8→9→10.
fn ten() -> (Spec, Plan) {
    let circles = vec![
        circle(0.0, 0.0, 20.0),
        circle(50.0, 0.0, 30.0),
        circle(50.0, 0.0, 15.0),
        circle(50.0, 35.0, 20.0),
        circle(50.0, 80.0, 25.0),
        circle(50.0, 80.0, 12.0),
        circle(80.0, 80.0, 18.0),
        circle(118.0, 80.0, 20.0),
        circle(118.0, 110.0, 10.0),
        circle(118.0, 136.0, 16.0),
    ];
    let d = Drawn { points: vec![], circles, lines: vec![] };
    let Reading::Train { circles: picked } = read(Some(Kind::Spur), &d).unwrap() else { panic!("not a train") };
    let cs: Vec<Circle> = picked.iter().map(|i| d.circles[*i]).collect();
    let settings = Simple { module: Some(2.0), ..Simple::default() };
    let (spec, _) = train_spec(&cs, Kind::Spur, &settings, &Spec::defaults_for(Kind::Spur)).unwrap();
    let p = plan(&spec).unwrap();
    (spec, p)
}

fn by_hand() -> Vec<f64> {
    let z = [20.0, 30.0, 15.0, 20.0, 25.0, 12.0, 18.0, 20.0, 10.0, 16.0];
    let mut w = vec![1.0; 10];
    w[1] = -z[0] / z[1];
    w[2] = w[1];
    w[3] = -w[2] * z[2] / z[3];
    w[4] = -w[3] * z[3] / z[4];
    w[5] = w[4];
    w[6] = -w[5] * z[5] / z[6];
    w[7] = -w[6] * z[6] / z[7];
    w[8] = -w[7] * z[7] / z[8];
    w[9] = -w[8] * z[8] / z[9];
    w
}

#[test]
fn a_compound_train_gives_every_ratio_its_path_and_stages() {
    let (spec, p) = ten();
    let g = |k: usize| member_of_gear(&p, k).unwrap();
    let want = by_hand();
    let e = Efficiencies::for_spec(&spec);
    for (k, &wk) in want.iter().enumerate().skip(1) {
        let r = plan_ratio_between(&spec, &p, g(0), g(k)).unwrap();
        assert!(close(r.ratio, wk), "gear {}: {} vs {wk}", k + 1, r.ratio);
        assert_eq!(r.direction, if wk > 0.0 { Direction::Same } else { Direction::Opposite });
        assert!(close(r.reduction, 1.0 / wk.abs()));
        assert!(r.mm_per_rev.is_none() && r.rev_per_mm.is_none());
    }
    // Gear 1 to gear 10: seven meshes (the clusters are one member each), all spur.
    let r = ratio_between(&p.assembly, g(0), g(9)).unwrap();
    assert_eq!(r.path.len(), 7, "{:?}", r.path);
    assert!(r.path.iter().all(|s| s.kind == StepKind::Mesh(MeshKind::Spur)));
    assert_eq!(r.path[0].teeth, Some((20, 30)));
    assert_eq!(r.path[1].teeth, Some((15, 20)), "across the compound: gear 3 drives gear 4");
    assert!(close(r.path.iter().map(|s| s.ratio).product(), r.ratio));
    assert!(close(r.efficiency, 0.99f64.powi(7)));
    // Backwards: the inverse.
    let back = ratio_between(&p.assembly, g(9), g(0)).unwrap();
    assert!(close(back.ratio * r.ratio, 1.0));
    // Stages and the overall ratio, against a target.
    let stages = stage_ratios(&p.assembly, g(0), g(9), &e).unwrap();
    assert_eq!(stages.len(), 7);
    let o = overall_ratio(&p.assembly, g(0), g(9), Some(1.0 / want[9].abs()), &e).unwrap();
    assert!(o.error.unwrap().abs() < 1e-9, "{:?}", o.note);
    assert!(o.note.unwrap().contains("exactly"));
    let o = overall_ratio(&p.assembly, g(0), g(9), Some(1.1 / want[9].abs()), &e).unwrap();
    assert!(close(o.error.unwrap(), 1.0 / 1.1 - 1.0));
    assert!(o.note.unwrap().contains("% off"));
    // Two gears of one cluster: the same member, an empty path.
    let same = ratio_between(&p.assembly, g(1), g(2)).unwrap();
    assert!(same.path.is_empty() && close(same.ratio, 1.0));
}

#[test]
fn the_drive_table_gives_speed_torque_and_power_at_every_member() {
    let (spec, p) = ten();
    let g = |k: usize| member_of_gear(&p, k).unwrap();
    let want = by_hand();
    let table = drive_table(&p.assembly, g(0), 100.0, 2.0, &Efficiencies::for_spec(&spec));
    assert_eq!(table.len(), p.assembly.members.len());
    let p_in = 2.0 * 100.0 * TAU / 60.0;
    for k in 0..10 {
        let st = &table[g(k)];
        assert!(st.reached && !st.locked, "{st:?}");
        assert!(close(st.rpm, 100.0 * want[k]), "gear {}: {}", k + 1, st.rpm);
        let eff = 0.99f64.powi(st.path.len() as i32);
        assert!(close(st.efficiency, eff));
        assert!(close(st.power, p_in * eff));
        // Torque grows with the reduction and is signed as the turn.
        assert!(close(st.torque, 2.0 * eff / want[k]), "gear {}: {}", k + 1, st.torque);
    }
    // Overridden: every mesh 0.9.
    let e = Efficiencies { per_link: (0..p.assembly.links.len()).map(|k| (k, 0.9)).collect(), ..Efficiencies::default() };
    let table = drive_table(&p.assembly, g(0), 100.0, 2.0, &e);
    assert!(close(table[g(9)].efficiency, 0.9f64.powi(7)));
}

#[test]
fn plan_mesh_checks_give_contact_ratio_and_hunting_tooth() {
    let (spec, p) = ten();
    let checks = plan_mesh_checks(&spec, &p);
    assert_eq!(checks.len(), 7);
    for c in &checks {
        let cr = c.contact_ratio.unwrap_or_else(|| panic!("{c:?}"));
        assert!(cr > 1.0 && cr < 2.5, "{c:?}");
        assert!(c.link.is_some(), "{c:?}");
    }
    // 20 and 30 teeth share 10; 18 and 20 share 2; 12 and 18 share 6.
    let one = checks.iter().find(|c| c.gears == Some((0, 1))).unwrap();
    assert_eq!((one.common_factor, one.hunting), (Some(10), Some(false)));
    assert!(one.notes.iter().any(|n| n.contains("factor of 10")), "{:?}", one.notes);
    let pair = Spec { teeth: 17, teeth2: 40, ..Spec::defaults_for(Kind::Pair) };
    let pp = plan(&pair).unwrap();
    let c = plan_mesh_checks(&pair, &pp);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].teeth, c[0].common_factor, c[0].hunting), (Some((17, 40)), Some(1), Some(true)));
    assert!(c[0].contact_ratio.unwrap() > 1.2, "{c:?}");
    assert_eq!(common_factor(48, 36), 12);
}

fn frame(origin: [f64; 3], z: [f64; 3]) -> Frame {
    let x = if z[2].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
    Frame { origin, x, z }
}

fn read_set(frames: &[Frame], teeth: &[u32], rings: &[usize], meshes: &[(usize, usize)], clusters: Vec<Vec<usize>>) -> Plan {
    let mut p = Plan::default();
    for (i, f) in frames.iter().enumerate() {
        p.bodies.push(crate::plan::Body { name: format!("g{}", i + 1), frame: *f, steps: Vec::new() });
        let mut m = crate::plan::SetMember { source: i, module: 2.0, teeth: teeth[i], ring: rings.contains(&i), body: Some(i), ..Default::default() };
        m.meshes_with = meshes
            .iter()
            .filter_map(|&(a, b)| {
                if a == i {
                    Some(b)
                } else if b == i {
                    Some(a)
                } else {
                    None
                }
            })
            .collect();
        m.cluster = clusters.iter().position(|c| c.contains(&i));
        p.members.push(m);
    }
    p.clusters = clusters;
    p
}

#[test]
fn a_bevel_compound_and_ring_graph_names_each_step() {
    let frames = [
        frame([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        frame([20.0, 0.0, 30.0], [-1.0, 0.0, 0.0]),
        frame([40.0, 0.0, 30.0], [-1.0, 0.0, 0.0]),
        frame([40.0, 25.0, 30.0], [-1.0, 0.0, 0.0]),
        frame([40.0, 25.0, 30.0], [-1.0, 0.0, 0.0]),
        frame([0.0, 40.0, 20.0], [0.0, 1.0, 0.0]),
    ];
    let teeth = [20, 30, 15, 10, 40, 16];
    let p = read_set(&frames, &teeth, &[4], &[(0, 1), (2, 3), (3, 4), (0, 5)], vec![vec![1, 2]]);
    let (a, issues) = crate::plan::assembly::from_members(&p).unwrap();
    assert!(issues.is_empty());
    let r = ratio_between(&a, 0, 4).unwrap();
    let kinds: Vec<StepKind> = r.path.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, vec![StepKind::Mesh(MeshKind::Bevel), StepKind::Shaft, StepKind::Mesh(MeshKind::Spur), StepKind::Mesh(MeshKind::Internal)]);
    let want = (-20.0 / 30.0) * (-15.0 / 10.0) * (10.0 / 40.0);
    assert!(close(r.ratio, want), "{}", r.ratio);
    assert!(close(r.efficiency, 0.97 * 0.99 * 0.99));
    // The ring turns the same way as the gear inside it.
    let ring = ratio_between(&a, 3, 4).unwrap();
    assert_eq!(ring.direction, Direction::Same);
    // A spiral bevel style names the bevel step so, at its efficiency.
    let e = Efficiencies { meeting: MeshKind::SpiralBevel, spiral_bevel: 0.98, ..Efficiencies::default() };
    let r = ratio_between_with(&a, 0, 1, &e).unwrap();
    assert_eq!(r.path[0].kind, StepKind::Mesh(MeshKind::SpiralBevel));
    assert!(close(r.efficiency, 0.98));
    // The face-gear case: g6's axis points away from the apex, both turn the same way; still a bevel.
    let r = ratio_between(&a, 0, 5).unwrap();
    assert_eq!((r.direction, r.path[0].kind), (Direction::Same, StepKind::Mesh(MeshKind::Bevel)));
    // A loop that can't turn (g4 also meshing g2): the plan reports it, the analysis refuses.
    let mut bad = read_set(&frames, &teeth, &[4], &[(0, 1), (2, 3), (3, 4), (0, 5), (1, 3)], vec![vec![1, 2]]);
    let (a, issues) = crate::plan::assembly::from_members(&bad).unwrap();
    assert_eq!(issues.len(), 1);
    bad.assembly = a;
    bad.assembly_issues = issues;
    let err = plan_ratio_between(&Spec::defaults_for(Kind::Spur), &bad, 0, 4).unwrap_err();
    assert!(err.contains("can't turn"), "{err}");
}

fn member(i: usize, origin: [f64; 3], dir: [f64; 3]) -> AssemblyMember {
    AssemblyMember { body: i, axis: Axis { origin, dir }, grounded: false, carrier: None }
}

#[test]
fn disconnected_members_and_disagreeing_loops_are_errors() {
    let z = [0.0, 0.0, 1.0];
    let members: Vec<AssemblyMember> = (0..4).map(|i| member(i, [40.0 * i as f64, 0.0, 0.0], z)).collect();
    let ext = |a, b, ra, rb| AssemblyLink::Gear { a, b, ra, rb, internal: false, reverse: false };
    let asm = Assembly { members: members.clone(), links: vec![ext(0, 1, 10.0, 20.0), ext(2, 3, 10.0, 20.0)] };
    let err = ratio_between(&asm, 0, 3).unwrap_err();
    assert!(err.contains("aren't connected"), "{err}");
    assert!(ratio_between(&asm, 0, 9).unwrap_err().contains("no member 10"));
    let t = drive_table(&asm, 0, 60.0, 1.0, &Efficiencies::default());
    assert!(t[1].reached && !t[2].reached && !t[3].reached);
    assert!(t[3].note.as_deref().unwrap().contains("not connected"));
    // Three outside meshes in a ring lock.
    let asm = Assembly { members: members[..3].to_vec(), links: vec![ext(0, 1, 10.0, 20.0), ext(1, 2, 20.0, 15.0), ext(2, 0, 15.0, 10.0)] };
    assert!(ratio_between(&asm, 0, 2).unwrap_err().contains("can't turn"));
    assert!(drive_table(&asm, 0, 60.0, 1.0, &Efficiencies::default()).iter().all(|s| s.locked));
    // A held member stops what it joins.
    let mut held = Assembly { members: members[..2].to_vec(), links: vec![ext(0, 1, 10.0, 20.0)] };
    held.members[1].grounded = true;
    assert!(ratio_between(&held, 0, 1).unwrap_err().contains("held"));
}

#[test]
fn a_rack_travels_its_pitch_circumference_per_turn() {
    let members = vec![member(0, [0.0; 3], [0.0, 0.0, 1.0]), member(1, [0.0, -10.0, 0.0], [1.0, 0.0, 0.0])];
    let asm = Assembly { members, links: vec![AssemblyLink::RackPinion { pinion: 0, rack: 1, pitch_radius: 10.0 }] };
    let r = ratio_between(&asm, 0, 1).unwrap();
    assert!(close(r.mm_per_rev.unwrap(), TAU * 10.0));
    assert_eq!(r.path[0].kind, StepKind::Mesh(MeshKind::Rack));
    let back = ratio_between(&asm, 1, 0).unwrap();
    assert!(close(back.rev_per_mm.unwrap(), 1.0 / (TAU * 10.0)));
    // 1 N·m at 60 rpm through a 10 mm pitch radius: 100 N less the loss, 62.8 mm/s.
    let t = drive_table(&asm, 0, 60.0, 1.0, &Efficiencies::default());
    assert!(t[1].linear && close(t[1].linear_speed.unwrap(), TAU * 10.0));
    assert!(close(t[1].force.unwrap(), 100.0 * 0.98), "{:?}", t[1].force);
    // Driven from the rack: 98 N at 62.8 mm/s turns the pinion at 60 rpm.
    let t = drive_table(&asm, 1, TAU * 10.0, 100.0, &Efficiencies::default());
    assert!(close(t[0].rpm, 60.0) && close(t[0].torque, 0.98));
    let o = overall_ratio(&asm, 0, 1, Some(TAU * 10.0), &Efficiencies::default()).unwrap();
    assert!(o.error.unwrap().abs() < 1e-9);
}

#[test]
fn worm_efficiency_and_self_locking() {
    // 1 start, q = 10: λ = 5.71°, μ 0.05: about 65 % forward, back-drivable.
    let w = Efficiencies::default().worm(1);
    assert!(close(w.lead_angle, 0.1f64.atan().to_degrees()));
    assert!((w.forward - 0.649).abs() < 0.002, "{w:?}");
    assert!(!w.self_locking && w.back > 0.4);
    // 2° lead: self-locking.
    let w = worm_efficiency(2.0, 0.05, 20.0);
    assert!(w.self_locking && w.back == 0.0 && w.forward > 0.3, "{w:?}");
    // The worm set as planned: wheel first, worm second.
    let s = Spec::defaults_for(Kind::Worm);
    let p = plan(&s).unwrap();
    let e = Efficiencies::for_spec(&s);
    let r = ratio_between_with(&p.assembly, 1, 0, &e).unwrap();
    assert!(close(r.ratio, -f64::from(s.starts.max(1)) / f64::from(s.teeth)));
    assert_eq!(r.path[0].kind, StepKind::Mesh(MeshKind::Worm));
    assert_eq!(r.path[0].teeth, Some((s.starts.max(1), s.teeth)));
    // A 2° lead: from the worm it drives; from the wheel it locks.
    let locking = Efficiencies { worm_lead_angle: Some(2.0), ..e.clone() };
    let t = drive_table(&p.assembly, 1, 1000.0, 1.0, &locking);
    assert!(!t[0].locked && t[0].torque.abs() > 0.0);
    assert!(close(t[0].rpm.abs(), 1000.0 * f64::from(s.starts.max(1)) / f64::from(s.teeth)));
    let t = drive_table(&p.assembly, 0, 10.0, 50.0, &locking);
    assert!(t[1].locked && t[1].rpm == 0.0, "{:?}", t[1]);
    assert!(t[1].note.as_deref().unwrap().contains("self-locking"));
    let r = ratio_between_with(&p.assembly, 0, 1, &locking).unwrap();
    assert!(r.path[0].self_locking && r.efficiency == 0.0 && r.notes.iter().any(|n| n.contains("self-locking")));
    // Rooted at the wheel, the link becomes a gear link (wheel → worm): a worm style still finds
    // the worm (the side with fewer teeth) and locks.
    let rooted = p.assembly.rooted(0);
    assert!(matches!(rooted.links[0], AssemblyLink::Gear { .. }));
    let t = drive_table(&rooted, 0, 10.0, 50.0, &locking);
    assert!(t[1].locked);
    assert!(!drive_table(&rooted, 1, 10.0, 1.0, &locking)[0].locked);
    let checks = mesh_checks(&p.assembly, &e);
    assert_eq!(checks[0].kind, MeshKind::Worm);
    assert_eq!(checks[0].hunting, Some(common_factor(s.starts.max(1), s.teeth) == 1));
}

#[test]
fn a_planetary_set_turns_its_ring_with_the_carrier_held() {
    let s = Spec::defaults_for(Kind::Planetary);
    let p = plan(&s).unwrap();
    let ring = p.assembly.members.len() - 1;
    let zr = if s.teeth3 > 0 { s.teeth3 } else { s.teeth + 2 * s.teeth2 };
    let r = plan_ratio_between(&s, &p, 0, ring).unwrap();
    assert!(close(r.ratio, -f64::from(s.teeth) / f64::from(zr)), "{}", r.ratio);
    assert_eq!(r.path.last().unwrap().kind, StepKind::Mesh(MeshKind::Internal));
    let checks = plan_mesh_checks(&s, &p);
    assert!(checks.iter().all(|c| c.contact_ratio.is_some()), "{checks:?}");
    assert!(checks.iter().any(|c| c.kind == MeshKind::Internal));
}

#[test]
fn styles_follow_the_spec() {
    assert_eq!(Efficiencies::for_spec(&Spec::defaults_for(Kind::Helical)).parallel, MeshKind::Helical);
    assert_eq!(Efficiencies::for_spec(&Spec::defaults_for(Kind::Herringbone)).parallel, MeshKind::Herringbone);
    let h = Efficiencies::for_spec(&Spec::defaults_for(Kind::Hypoid));
    assert_eq!(h.skew, MeshKind::Hypoid);
    assert!(close(h.of(MeshKind::Hypoid, 1), 0.95));
    assert_eq!(Efficiencies::for_spec(&Spec::defaults_for(Kind::Sprocket)).parallel_same, MeshKind::Wrap);
    // A crossed-helical pair as planned: skew shafts.
    let s = Spec::defaults_for(Kind::CrossedHelical);
    let p = plan(&s).unwrap();
    let r = ratio_between_with(&p.assembly, 0, 1, &Efficiencies::for_spec(&s)).unwrap();
    assert_eq!(r.path[0].kind, StepKind::Mesh(MeshKind::CrossedHelical));
    assert!(close(r.efficiency, 0.9));
}
