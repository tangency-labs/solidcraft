//! A whole gear system as one assembly: a train of ten gears with two compound clusters drawn
//! as circles, driven from any member; its mesh graph's loops; one keep-out for everything.

use crate::clearance::Clearance;
use crate::mesh2d::{Verdict, check_mesh};
use crate::plan::{Assembly, AssemblyLink, AssemblyMember, Kind, Plan, Spec};
use crate::simple::Simple;
use crate::sketch_rules::{Circle, Drawn, Reading, read, train_spec};
use crate::{V2, axes::Axis};

fn circle(x: f64, y: f64, r: f64) -> Circle {
    Circle { c: V2::new(x, y), r }
}

/// Ten gears, module 2 (radius = teeth): 1 drives 2; 2 and 3 share a shaft; 3 → 4 → 5; 5 and 6
/// share a shaft; 6 → 7 → 8 → 9 → 10. Gears off each other's layers may cross circles.
fn ten() -> Vec<Circle> {
    vec![
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
    ]
}

/// Each gear's turn per turn of gear 1, worked out by hand from the teeth: the other way across
/// every outside mesh by the teeth's ratio, the same on a shared shaft.
fn by_hand() -> Vec<f64> {
    let z = [20.0, 30.0, 15.0, 20.0, 25.0, 12.0, 18.0, 20.0, 10.0, 16.0];
    let mut w = vec![1.0; 10];
    w[1] = -w[0] * z[0] / z[1];
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

fn system(clearance: bool) -> (Spec, Plan) {
    let d = Drawn { points: vec![], circles: ten(), lines: vec![] };
    let Reading::Train { circles: picked } = read(Some(Kind::Spur), &d).unwrap_or_else(|e| panic!("{e}")) else { panic!("not read as a train") };
    let cs: Vec<Circle> = picked.iter().map(|i| d.circles[*i]).collect();
    let settings = Simple { module: Some(2.0), ..Simple::default() };
    let (mut spec, _) = train_spec(&cs, Kind::Spur, &settings, &Spec::defaults_for(Kind::Spur)).unwrap_or_else(|e| panic!("{e}"));
    if clearance {
        spec.clearance_body = Some(Clearance::default());
    }
    let p = crate::plan(&spec).unwrap_or_else(|e| panic!("{e}"));
    (spec, p)
}

/// The member that is gear `k` of the drawing (bodies come in the drawing's order, but say so).
fn gear_teeth(spec: &Spec) -> Vec<u32> {
    spec.train.as_ref().unwrap().gears.iter().map(|g| g.teeth).collect()
}

use crate::test_util::close_rel as close;

/// The body (assembly member) gear `k` of the drawing is on: one body per member that turns as
/// one (a compound cluster's gears and its shaft joined).
fn body_of(p: &Plan, k: usize) -> usize {
    p.members[k].body.unwrap_or_else(|| panic!("gear {} has no body", k + 1))
}

#[test]
fn a_ten_gear_train_with_two_compound_clusters_turns_every_gear_by_its_ratio() {
    let (spec, p) = system(false);
    assert_eq!(gear_teeth(&spec), vec![20, 30, 15, 20, 25, 12, 18, 20, 10, 16], "the drawing's gears, in order");
    let a = &p.assembly;
    // One member per shaft: eight; each cluster's two gears on one body.
    assert_eq!(a.members.len(), 8);
    assert_eq!((body_of(&p, 1), body_of(&p, 4)), (body_of(&p, 2), body_of(&p, 5)), "compounds share a body");
    assert!(p.assembly_issues.is_empty(), "{:?}", p.assembly_issues);
    assert!(!a.links.iter().any(|l| matches!(l, AssemblyLink::Rigid { .. })), "no rigid links: a cluster is one body");
    assert_eq!(a.links.len(), 7, "a gear link per mesh, a tree over eight members");
    // Gear 1 turned by 30°: every gear where the teeth say.
    let want = by_hand();
    let theta = 30f64.to_radians();
    let g1 = body_of(&p, 0);
    for (k, w) in want.iter().enumerate() {
        let got = a.ratio(g1, body_of(&p, k)).unwrap_or_else(|| panic!("gear {} is not linked", k + 1)) * theta;
        assert!(close(got, w * theta), "gear {}: {got} rad, by hand {}", k + 1, w * theta);
    }
    // Driven from gear 7 instead: the same motion, gear 7's turn as the unit.
    let g7 = body_of(&p, 6);
    let from7 = a.rooted(g7);
    assert_eq!(from7.links.len(), 7);
    for k in 0..10 {
        let got = from7.ratio(g7, body_of(&p, k)).unwrap();
        assert!(close(got, want[k] / want[6]), "gear {} from gear 7: {got} vs {}", k + 1, want[k] / want[6]);
    }
    // Rooted from gear 7, every link points away from it: each member is driven once, and not
    // gear 7's.
    let mut driven = vec![false; a.members.len()];
    for l in &from7.links {
        let (_, b) = l.members();
        assert!(!driven[b] && b != g7, "{l:?}");
        driven[b] = true;
    }
}

#[test]
fn the_ten_gear_train_stays_in_mesh_as_the_assembly_turns_it() {
    let (_, mut p) = system(false);
    // The 2D checker turns each gear (a preview member each) by its preview speed: give it the
    // assembly's ratios, and it probes every mesh through a tooth pitch of its driver for
    // overlap and lost contact.
    assert_eq!(p.preview.len(), 10);
    let g1 = body_of(&p, 0);
    for k in 0..p.preview.len() {
        let r = p.assembly.ratio(g1, body_of(&p, k)).unwrap();
        assert!(close(r, p.preview[k].speed), "gear {}: assembly {r}, preview {}", k + 1, p.preview[k].speed);
        p.preview[k].speed = r;
    }
    let results = check_mesh(&p);
    assert_eq!(results.len(), 7, "seven meshes (the two shared shafts aren't meshes)");
    for m in &results {
        assert_eq!(m.verdict, Verdict::Ok, "{}", m.message);
    }
    // Turned the wrong way, a gear jams: the checker sees it.
    p.preview[3].speed = -p.preview[3].speed;
    assert!(check_mesh(&p).iter().any(|m| m.verdict != Verdict::Ok), "a gear turned against its mate must interfere");
}

fn member(i: usize) -> AssemblyMember {
    AssemblyMember { body: i, axis: Axis { origin: [40.0 * i as f64, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }, grounded: false, carrier: None }
}

fn ext(a: usize, b: usize, ra: f64, rb: f64) -> AssemblyLink {
    AssemblyLink::Gear { a, b, ra, rb, internal: false, reverse: false }
}

#[test]
fn a_loop_whose_ratios_dont_agree_is_reported_per_member() {
    // Three gears meshing in a ring: an odd number of outside meshes locks.
    let members: Vec<AssemblyMember> = (0..3).map(member).collect();
    let edges = vec![ext(0, 1, 10.0, 20.0), ext(1, 2, 20.0, 15.0), ext(2, 0, 15.0, 10.0)];
    let (a, issues) = Assembly::from_graph(members.clone(), &edges, 0);
    assert_eq!(a.links.len(), 2, "the closing mesh is left out");
    assert_eq!(issues.len(), 1, "{issues:?}");
    let issue = &issues[0];
    let mut round = issue.members.clone();
    round.sort_unstable();
    assert_eq!(round, vec![0, 1, 2], "{issue:?}");
    for i in 1..=3 {
        assert!(issue.message.contains(&format!("member {i}")), "{}", issue.message);
    }
    assert!(!close(issue.given, issue.asked));
    // The same ring with the third mesh inside a ring gear agrees: no issue.
    let edges =
        vec![ext(0, 1, 10.0, 20.0), ext(1, 2, 20.0, 30.0), AssemblyLink::Gear { a: 0, b: 2, ra: 10.0, rb: 30.0, internal: true, reverse: false }];
    let (a, issues) = Assembly::from_graph(members.clone(), &edges, 0);
    assert!(issues.is_empty(), "{issues:?}");
    assert!(close(a.ratio(0, 2).unwrap(), 10.0 / 30.0));
    // A chain of four with ratios that don't multiply to 1 round the loop.
    let members: Vec<AssemblyMember> = (0..4).map(member).collect();
    let edges = vec![ext(0, 1, 10.0, 20.0), ext(1, 2, 20.0, 10.0), ext(2, 3, 10.0, 20.0), ext(3, 0, 25.0, 10.0)];
    let (_, issues) = Assembly::from_graph(members, &edges, 0);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].members.len(), 4, "{:?}", issues[0]);
}

#[test]
fn reversed_links_turn_back_by_the_inverse_ratio() {
    let worm = AssemblyLink::Worm { worm: 1, wheel: 0, starts: 2, wheel_teeth: 30 };
    let gear = ext(0, 1, 10.0, 25.0);
    let ring = AssemblyLink::Gear { a: 0, b: 1, ra: 10.0, rb: 40.0, internal: true, reverse: false };
    for l in [worm, gear, ring, AssemblyLink::Rigid { a: 0, b: 1 }] {
        let back = l.reversed();
        assert_eq!(back.members(), (l.members().1, l.members().0), "{l:?}");
        assert!(close(back.ratio() * l.ratio(), 1.0), "{l:?}");
    }
}

/// One keep-out for the whole system, built once over every member: each gear's material,
/// turned through its motion, lies inside it with the clearances, wherever gears meet.
#[test]
fn one_keep_out_holds_the_whole_ten_gear_system() {
    let (spec, p) = system(true);
    let keep = p.clearance.as_ref().expect("a keep-out body");
    assert!(keep.name.ends_with(" clearance"), "{}", keep.name);
    assert_eq!(p.assembly.members.len(), p.bodies.len(), "the keep-out is not a member");
    let tried = crate::clearance_tests::contains_every_member(&spec, &p, 0.01);
    assert!(tried > 10 * 1000, "only {tried} points tried");
}

/// A plan as a sketch reader leaves it: bodies placed on their shafts, and the members' mesh
/// graph and clusters ([`Plan::members`], [`Plan::clusters`]).
fn read_set(frames: &[crate::plan::Frame], teeth: &[u32], rings: &[usize], meshes: &[(usize, usize)], clusters: Vec<Vec<usize>>) -> Plan {
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

fn frame(origin: [f64; 3], z: [f64; 3]) -> crate::plan::Frame {
    let x = if z[2].abs() > 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
    crate::plan::Frame { origin, x, z }
}

#[test]
fn a_3d_sketch_system_of_bevels_compounds_and_a_ring_turns_by_its_graph() {
    // g1 on +Z at the origin meshes g2, a bevel on −X at (20, 0, 30): both point at the apex
    // (0, 0, 30). g3 shares g2's shaft further out; g4 meshes g3 on a parallel shaft; g5 is a
    // ring round g4 (same direction), and g6 a bevel whose shaft points away from its apex with
    // g1 (it turns the same way: the face-gear case).
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
    let (a, issues) = crate::plan::assembly::from_members(&p).expect("a graph");
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(a.links.len(), 5);
    let w = |i: usize| a.ratio(0, i).unwrap();
    assert!(close(w(1), -20.0 / 30.0), "bevel: {}", w(1));
    assert!(close(w(2), w(1)), "compound: {}", w(2));
    assert!(close(w(3), -w(2) * 15.0 / 10.0), "parallel: {}", w(3));
    assert!(close(w(4), w(3) * 10.0 / 40.0), "ring: {}", w(4));
    // g6's axis from (0, 40, 20) along +Y points away from where it meets g1's (0, 0, 20).
    assert!(close(w(5), 20.0 / 16.0), "away from the apex: {}", w(5));
    // Every gear of a cluster is rigid on the cluster's first.
    assert!(a.links.contains(&AssemblyLink::Rigid { a: 1, b: 2 }));
    // A loop that can't turn: g4 also meshing g2 (outside), against g3's path.
    let p = read_set(&frames, &teeth, &[4], &[(0, 1), (2, 3), (3, 4), (0, 5), (1, 3)], vec![vec![1, 2]]);
    let (_, issues) = crate::plan::assembly::from_members(&p).unwrap();
    assert_eq!(issues.len(), 1, "{issues:?}");
    let mut round = issues[0].members.clone();
    round.sort_unstable();
    assert_eq!(round, vec![1, 2, 3], "{:?}", issues[0]);
}

/// The user's gearbox: a Ø30 input drives two Ø15 countershaft gears (twin countershafts); each
/// countershaft carries a Ø25 beside its Ø15 (a compound), and both Ø25s drive one Ø15 output.
/// A closed loop of meshes with equal branches (a power split): it agrees, so it is no issue;
/// the joints are one revolute per shaft (input, countershaft A, countershaft B, output), the
/// clusters rigid, and the mesh that closes the loop is left out of the links (the joints'
/// motion links take each driven joint once) and holds by the ratios.
fn gearbox(b25_teeth: u32) -> Plan {
    let z = [0.0, 0.0, 1.0];
    let frames = [
        frame([0.0, 0.0, 0.0], z),
        frame([22.5, 0.0, 0.0], z),
        frame([22.5, 0.0, 15.0], z),
        frame([-22.5, 0.0, 0.0], z),
        frame([-22.5, 0.0, 15.0], z),
        frame([0.0, 15.0, 15.0], z),
    ];
    // Module 1: the diameters are the teeth.
    let teeth = [30, 15, 25, 15, b25_teeth, 15];
    read_set(&frames, &teeth, &[], &[(0, 1), (0, 3), (2, 5), (4, 5)], vec![vec![1, 2], vec![3, 4]])
}

#[test]
fn a_twin_countershaft_gearbox_splits_power_round_a_loop_that_agrees() {
    let p = gearbox(25);
    let (a, issues) = crate::plan::assembly::from_members(&p).expect("a graph");
    assert!(issues.is_empty(), "a power split is no over-constraint: {issues:?}");
    // Four shafts: the members no rigid link drives.
    let rigid_driven: Vec<usize> = a.links.iter().filter(|l| matches!(l, AssemblyLink::Rigid { .. })).map(|l| l.members().1).collect();
    assert_eq!(rigid_driven.len(), 2);
    assert_eq!(6 - rigid_driven.len(), 4, "input, two countershafts, output");
    let gear_links = a.links.iter().filter(|l| matches!(l, AssemblyLink::Gear { .. })).count();
    assert_eq!(gear_links, 3, "three of the four meshes linked, the fourth closes the loop");
    // The input by θ: each countershaft −2θ, the output +(2·25/15)θ, by both branches.
    let theta = 0.3;
    for (i, want) in [(1, -2.0), (2, -2.0), (3, -2.0), (4, -2.0), (5, 2.0 * 25.0 / 15.0)] {
        let got = a.ratio(0, i).unwrap() * theta;
        assert!(close(got, want * theta), "member {}: {got} vs {}", i + 1, want * theta);
    }
    // The mesh left out holds: across it the output turns what the other branch gives it.
    let both: Vec<f64> = [(2usize, 25.0), (4usize, 25.0)].iter().map(|(k, zk)| -a.ratio(0, *k).unwrap() * zk / 15.0).collect();
    assert!(close(both[0], both[1]) && close(both[0], a.ratio(0, 5).unwrap()), "{both:?}");
    // Driven from the output instead: the same, the output's turn as the unit.
    let back = a.rooted(5);
    assert!(close(back.ratio(5, 0).unwrap(), 15.0 / (2.0 * 25.0)));
}

#[test]
fn a_gearbox_whose_branches_disagree_is_reported() {
    // Countershaft B's big gear has 24 teeth: the output can't turn at both branches' speeds.
    let p = gearbox(24);
    let (a, issues) = crate::plan::assembly::from_members(&p).unwrap();
    assert_eq!(issues.len(), 1, "{issues:?}");
    let mut round = issues[0].members.clone();
    round.sort_unstable();
    assert_eq!(round, vec![0, 1, 2, 3, 4, 5], "the whole loop: input, both countershafts, output: {:?}", issues[0]);
    assert!(issues[0].message.contains("member 6"), "{}", issues[0].message);
    // What is linked still turns: the input drives everything along the tree.
    for i in 1..6 {
        assert!(a.ratio(0, i).is_some(), "member {}", i + 1);
    }
}

/// The user's gearbox drawn as circles (module 1: diameter = teeth): a Ø30 input, two Ø15
/// countershaft gears at ±30° round it, a Ø25 beside each on its countershaft, and one Ø15
/// output meshing both Ø25s. The loop agrees: no issue; the input by θ turns each countershaft
/// −2θ and the output +(2·25/15)θ; every mesh checks clean at the assembled zero and through a
/// tooth pitch.
pub(crate) fn gaerbox_circles() -> Vec<Circle> {
    // ±45.98° round the input: the two Ø25s clear each other, the output reaches both, and the
    // loop's teeth close (the train planner refuses a loop that isn't a whole number of teeth).
    let a = 45.98f64.to_radians();
    let (cx, cy) = (22.5 * a.cos(), 22.5 * a.sin());
    let x0 = cx + (400.0 - cy * cy).sqrt();
    vec![circle(0.0, 0.0, 15.0), circle(cx, cy, 7.5), circle(cx, cy, 12.5), circle(cx, -cy, 7.5), circle(cx, -cy, 12.5), circle(x0, 0.0, 7.5)]
}

#[test]
fn the_gaerbox_drawn_as_circles_turns_and_meshes() {
    let d = Drawn { points: vec![], circles: gaerbox_circles(), lines: vec![] };
    let Reading::Train { circles: picked } = read(Some(Kind::Spur), &d).unwrap_or_else(|e| panic!("{e}")) else { panic!("not read as a train") };
    let cs: Vec<Circle> = picked.iter().map(|i| d.circles[*i]).collect();
    let settings = Simple { module: Some(1.0), ..Simple::default() };
    let (spec, _) = train_spec(&cs, Kind::Spur, &settings, &Spec::defaults_for(Kind::Spur)).unwrap_or_else(|e| panic!("{e}"));
    let p = crate::plan(&spec).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(gear_teeth(&spec), vec![30, 15, 25, 15, 25, 15]);
    assert!(p.assembly_issues.is_empty(), "{:?}", p.assembly_issues);
    let a = &p.assembly;
    // Four members (input, countershaft A, countershaft B, output), three gear links: the fourth
    // mesh closes the loop and agrees.
    assert_eq!(a.members.len(), 4, "{:?}", p.members);
    assert_eq!(a.links.len(), 3);
    let input = body_of(&p, 0);
    for (k, want) in [(1, -2.0), (2, -2.0), (3, -2.0), (4, -2.0), (5, 2.0 * 25.0 / 15.0)] {
        let got = a.ratio(input, body_of(&p, k)).unwrap();
        assert!(close(got, want), "gear {}: {got} vs {want}", k + 1);
    }
    for m in &check_mesh(&p) {
        assert_eq!(m.verdict, Verdict::Ok, "{}", m.message);
    }
    assert_eq!(check_mesh(&p).len(), 4, "four meshes");
}
