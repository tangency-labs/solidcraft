//! How the pairs of a drivetrain of items turn, checked against their teeth (not only the ratio's
//! size): the assembly's turn of the mate per turn of the driver, put on the planned bodies,
//! keeps the teeth in mesh (no penetration over a stretch of the turn, the teeth in the gaps at
//! rest), while the opposite sign drives them into each other. Drawn as the owner's 4×4: a prop
//! line bending 20° into a bevel pair (160° between the axes pointed at the apex), and a hypoid
//! whose pinion lies along the bent line, the rear one the front's mirror image.

use crate::items::{GearItem, GearRef, GearSettings, ItemKind, ItemLink, LinkKind, SizeBy, Thing};
use crate::plan::system::SystemPlan;
use crate::test_util::{bend, g, planned_system as planned};

fn teeth(z1: u32, z2: u32, module: Option<f64>) -> Vec<GearSettings> {
    let s = |z| GearSettings { size_by: SizeBy::Teeth, teeth: Some(z), module, ..GearSettings::default() };
    vec![s(z1), s(z2)]
}

fn bevel(x: f64) -> GearItem {
    let mut it = GearItem::new("K", ItemKind::Bevel).with(vec![Thing::line([0.0; 3], [x, 0.0, 0.0]), Thing::line([x, 0.0, 0.0], bend(x))]);
    it.gears = teeth(30, 30, Some(0.6));
    it
}

/// The hypoid at the end of the bent line: the ring (Ø50, axis y) at (±125, 0, 20), the pinion
/// along the bent line.
fn hypoid(x: f64) -> GearItem {
    let mut it = GearItem::new("H", ItemKind::Hypoid)
        .with(vec![Thing::circle([x.signum() * 125.0, 0.0, 20.0], [0.0, 1.0, 0.0], 25.0), Thing::line([x, 0.0, 0.0], bend(x))]);
    it.gears = teeth(37, 9, None);
    it
}

/// Turn of body `b` about its frame's z per turn of body `a` about its frame's z, as the
/// assembly has it (the assembly turns each member about its own axis, which may point against
/// its frame's z).
fn spin_ratio(sp: &SystemPlan, a: usize, b: usize) -> f64 {
    let asm = &sp.plan.assembly;
    let r = asm.ratio(a, b).unwrap_or_else(|| panic!("no ratio {a} → {b}"));
    let sign = |k: usize| {
        let d = asm.members[k].axis.dir;
        let z = sp.plan.bodies[k].frame.z;
        if d[0] * z[0] + d[1] * z[1] + d[2] * z[2] < 0.0 { -1.0 } else { 1.0 }
    };
    r * sign(a) * sign(b)
}

/// The deepest penetration of the pair turned `ratio` (frame spins) over `turn` radians of `a`
/// from rest, in `steps` steps, and at rest.
fn run(sp: &SystemPlan, a: usize, b: usize, ratio: f64, turn: f64, steps: usize) -> (f64, f64) {
    let (ba, bb) = (&sp.plan.bodies[a], &sp.plan.bodies[b]);
    let mut pair = crate::mesh3d::Pair::for_run(ba, bb, 0.05, 0.2, ratio, -turn, turn);
    let rest = pair.at(0.0, 0.0).penetration;
    let r = pair.run(ratio, 0.0, -turn, turn, steps);
    (r.penetration, rest)
}

/// A pair's check: at rest the teeth sit in the gaps, the assembly's turn keeps them in mesh,
/// and the other sign runs them into each other (so the check can tell).
fn meshes(sp: &SystemPlan, r0: GearRef, r1: GearRef, module: f64) {
    let (a, b) = (sp.body(r0).unwrap(), sp.body(r1).unwrap());
    let ratio = spin_ratio(sp, a, b);
    let turn = 1.5 * std::f64::consts::TAU / 30.0;
    let (good, rest) = run(sp, a, b, ratio, turn, 5);
    let (bad, _) = run(sp, a, b, -ratio, turn, 5);
    let tol = 0.05 * module;
    assert!(rest <= tol, "at rest the teeth overlap {rest} mm: their phase is wrong");
    assert!(good <= tol, "turned as the assembly turns them (ratio {ratio}) the teeth run {good} mm into each other");
    assert!(bad > 4.0 * tol, "turned the other way the teeth run only {bad} mm in: the check can't tell the sign");
}

#[test]
fn a_bevel_pair_at_a_20_degree_bend_meshes_and_turns_the_right_way() {
    for x in [80.0, -80.0] {
        let sp = planned(&[bevel(x)], &[]);
        meshes(&sp, g(0, 0), g(0, 1), 0.6);
        // 30 : 30, so 1 : 1 in size.
        let r = sp.plan.assembly.ratio(sp.body(g(0, 0)).unwrap(), sp.body(g(0, 1)).unwrap()).unwrap();
        assert!((r.abs() - 1.0).abs() < 1e-9, "{r}");
    }
}

#[test]
fn a_hypoid_on_the_bent_line_turns_its_ring_the_way_its_pinion_drives_it() {
    for x in [80.0, -80.0] {
        let sp = planned(&[hypoid(x)], &[]);
        let m = sp.gears.iter().find(|r| r.item == 0 && r.gear == 0).map(|r| r.module).unwrap();
        meshes(&sp, g(0, 1), g(0, 0), m);
    }
}

#[test]
fn front_and_rear_axles_turn_the_same_way_about_y() {
    // Prop shaft along x (both bevels' first gears on it), each bend's mate carries its hypoid's
    // pinion: the rings must turn the same way about +y, so the car goes one way.
    let mut prop = GearItem::new("P", ItemKind::Shaft).with(vec![Thing::line([-80.0, 0.0, 0.0], [80.0, 0.0, 0.0])]);
    prop.shaft_diameter = Some(6.0);
    let items = vec![bevel(80.0), bevel(-80.0), hypoid(80.0), hypoid(-80.0), prop];
    let sh = |a: GearRef, b: GearRef| ItemLink { a, b, kind: LinkKind::Shaft };
    let links = vec![sh(g(4, 0), g(0, 0)), sh(g(4, 0), g(1, 0)), sh(g(0, 1), g(2, 1)), sh(g(1, 1), g(3, 1))];
    let sp = planned(&items, &links);
    let p = sp.body(g(4, 0)).unwrap();
    let about_y = |r: GearRef| {
        let b = sp.body(r).unwrap();
        let d = sp.plan.assembly.members[b].axis.dir;
        sp.plan.assembly.ratio(p, b).unwrap() * d[1].signum()
    };
    let (front, rear) = (about_y(g(2, 0)), about_y(g(3, 0)));
    assert!((front.abs() - rear.abs()).abs() < 1e-9, "{front} {rear}");
    assert!(front * rear > 0.0, "the front axle turns {front} about +y per turn of the prop shaft, the rear {rear}");
}
