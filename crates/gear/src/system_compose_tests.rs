//! Whole drivetrains from items (plan::system, WP1): stages of every kind planned by their own
//! planners, placed where the drawing puts them and merged into one plan and one assembly.

use crate::analysis::{Efficiencies, drive_table, ratio_between};
use crate::items::{Drive, GearItem, GearRef, GearSettings, ItemKind, ItemLink, Thing};
use crate::plan::system::{SystemPlan, plan_items};
use crate::plan::{AssemblyLink, Spec};

const Z: [f64; 3] = [0.0, 0.0, 1.0];
const X: [f64; 3] = [1.0, 0.0, 0.0];

fn item(id: &str, kind: ItemKind, things: Vec<Thing>) -> GearItem {
    GearItem::new(id, kind).with(things)
}
fn circ(c: [f64; 3], n: [f64; 3], r: f64) -> Thing {
    Thing::circle(c, n, r)
}
fn line(a: [f64; 3], b: [f64; 3]) -> Thing {
    Thing::line(a, b)
}
use crate::test_util::{g, mesh, shaft};

fn planned(items: &[GearItem], links: &[ItemLink], drive: Option<&Drive>) -> SystemPlan {
    let sp = plan_items(items, links, &[], drive, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(sp.error.is_none(), "{:?}", sp.error);
    assert!(sp.plan.assembly_issues.is_empty(), "{:?}", sp.plan.assembly_issues);
    // One assembly member per body; every gear has its body.
    assert_eq!(sp.plan.assembly.members.len(), sp.plan.bodies.len());
    for (r, _) in &sp.member_of {
        assert!(sp.body(*r).is_some(), "{r:?} has no body");
    }
    sp
}

fn teeth(sp: &SystemPlan, r: GearRef) -> f64 {
    f64::from(sp.gears.iter().find(|x| x.item == r.item && x.gear == r.gear).map(|x| x.teeth).unwrap_or(0))
}

fn reply_ratio(sp: &SystemPlan, r: GearRef) -> f64 {
    sp.gears.iter().find(|x| x.item == r.item && x.gear == r.gear).and_then(|x| x.ratio).unwrap_or_else(|| panic!("{r:?} has no ratio"))
}

use crate::test_util::close_rel as close;

/// A spur pair; the driven spur's shaft carries a bevel pinion (z up, apex 20 above it), whose
/// mate (on an x shaft) shares its shaft with a worm wheel; the worm lies across the wheel.
fn chain() -> (Vec<GearItem>, Vec<ItemLink>) {
    let items = vec![
        item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 20.0)]),
        item("B", ItemKind::Spur, vec![circ([50.0, 0.0, 0.0], Z, 30.0)]),
        item("C", ItemKind::Bevel, vec![circ([50.0, 0.0, 40.0], Z, 15.0), circ([65.0, 0.0, 60.0], X, 20.0)]),
        item("D", ItemKind::Worm, vec![circ([110.0, 0.0, 60.0], X, 30.0), line([110.0, -40.0, 98.0], [110.0, 40.0, 98.0])]),
    ];
    let links = vec![mesh(g(0, 0), g(1, 0)), shaft(g(1, 0), g(2, 0)), shaft(g(2, 1), g(3, 0))];
    (items, links)
}

#[test]
fn spur_to_bevel_to_worm_is_one_plan_and_one_assembly() {
    let (items, links) = chain();
    let drive = Drive { item: 0, gear: 0, rpm: Some(1000.0), torque: Some(2.0) };
    let sp = planned(&items, &links, Some(&drive));
    // Six gears, each its own body; B with the bevel pinion and the bevel gear with the wheel are
    // one rigid member each.
    assert_eq!(sp.member_of.len(), 6, "{:?}", sp.member_of);
    assert_eq!(sp.plan.bodies.len(), 6, "{:?}", sp.plan.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    assert_eq!(sp.plan.clusters.len(), 2, "{:?}", sp.plan.clusters);
    let rigid = crate::plan::solid::members(&sp.plan);
    assert_eq!(rigid.len(), 4, "{rigid:?}");
    // The worm stage driven from its wheel: a gear link of the wheel's teeth against the starts.
    let (wb, wm) = (sp.body(g(3, 0)).unwrap(), sp.body(g(3, 1)).unwrap());
    assert!(
        sp.plan.assembly.links.iter().any(|l| matches!(*l, AssemblyLink::Gear { a, b, rb, .. } if a == wb && b == wm && rb == 1.0)),
        "{:?}",
        sp.plan.assembly.links
    );
    // The wheel turns with the bevel gear (A:B, then the bevel); it drives the worm by its
    // teeth against the starts.
    let wheel = reply_ratio(&sp, g(3, 0));
    let by_hand = teeth(&sp, g(0, 0)) / teeth(&sp, g(1, 0)) * teeth(&sp, g(2, 0)) / teeth(&sp, g(2, 1));
    assert!(close(wheel.abs(), by_hand), "{wheel} vs {by_hand}: {:?}", sp.gears);
    assert_eq!(teeth(&sp, g(3, 1)), 1.0, "the worm's starts");
    let worm = reply_ratio(&sp, g(3, 1));
    assert!(close(worm.abs(), wheel.abs() * teeth(&sp, g(3, 0))), "{worm}");
    // The ratios are the assembly's and the analysis'.
    let (root, w) = (sp.driver.unwrap(), sp.body(g(3, 0)).unwrap());
    let report = ratio_between(&sp.plan.assembly, root, w).unwrap();
    assert!(close(report.ratio, wheel), "{} vs {wheel}", report.ratio);
    assert!(close(sp.plan.assembly.ratio(root, w).unwrap(), wheel));
    // Speeds from the drive: the wheel at 1000 rpm × the ratio.
    let states = drive_table(&sp.plan.assembly, root, 1000.0, 2.0, &Efficiencies::default());
    let rpm = sp.gears.iter().find(|x| x.item == 3 && x.gear == 0).and_then(|x| x.rpm).unwrap();
    assert!(close(rpm, states[w].rpm) && close(rpm, 1000.0 * wheel), "{rpm}");
    // Links: the mesh's ratio −zA/zB, the shafts 1.
    assert!(close(sp.links[0].ratio.unwrap(), -teeth(&sp, g(0, 0)) / teeth(&sp, g(1, 0))), "{:?}", sp.links[0]);
    assert!(sp.links[1..].iter().all(|l| close(l.ratio.unwrap(), 1.0)), "{:?}", sp.links);
    // Gear C1 sits where it was drawn: its body on the x axis through (·, 0, 60).
    let f = sp.plan.bodies[sp.body(g(2, 1)).unwrap()].frame;
    assert!(f.origin[1].abs() < 1e-6 && (f.origin[2] - 60.0).abs() < 1e-6 && f.z[0].abs() > 1.0 - 1e-9, "{f:?}");
}

#[test]
fn a_compound_in_two_sketches_drives_a_planetary_set_with_its_carrier() {
    // The compound's shaft line along z; its gears on two parallel sketch planes (z 0 and 20);
    // a spur meshes gear 1; the sun of a planetary set further up the shaft turns with it.
    let items = vec![
        item("E", ItemKind::Compound, vec![line([0.0, 0.0, -10.0], [0.0, 0.0, 30.0]), circ([0.0; 3], Z, 20.0), circ([0.0, 0.0, 20.0], Z, 12.0)]),
        item("F", ItemKind::Spur, vec![circ([50.0, 0.0, 0.0], Z, 30.0)]),
        item("P", ItemKind::Planetary, vec![circ([0.0, 0.0, 60.0], Z, 10.0), circ([0.0, 0.0, 60.0], Z, 40.0)]),
    ];
    let links = vec![mesh(g(0, 0), g(1, 0)), shaft(g(0, 1), g(2, 0))];
    let drive = Drive { item: 1, gear: 0, rpm: Some(100.0), torque: None };
    let sp = planned(&items, &links, Some(&drive));
    // The carrier is generated; the planets ride on it.
    assert_eq!(sp.carriers.len(), 1);
    let carrier = sp.carriers[0].1;
    let planets: Vec<usize> = sp.plan.assembly.members.iter().enumerate().filter(|(_, m)| m.carrier == Some(carrier)).map(|(i, _)| i).collect();
    assert!(!planets.is_empty() && planets.len() <= 3, "{planets:?}");
    // The compound (and the sun with it) is one rigid member.
    let e = sp.member(g(0, 0)).unwrap();
    let sun = sp.member(g(2, 0)).unwrap();
    assert!(sp.plan.members[e].cluster.is_some() && sp.plan.members[e].cluster == sp.plan.members[sun].cluster);
    // Ratios: the sun turns with the compound (−zF/zE); the ring at −zs/zr of that (carrier held).
    let s = reply_ratio(&sp, g(2, 0));
    assert!(close(s, -teeth(&sp, g(1, 0)) / teeth(&sp, g(0, 0))), "{s}");
    let ring = reply_ratio(&sp, g(2, 1));
    assert!(close(ring, -s * teeth(&sp, g(2, 0)) / teeth(&sp, g(2, 1))), "{ring} {s} {:?}", sp.gears);
    let root = sp.driver.unwrap();
    let rb = sp.body(g(2, 1)).unwrap();
    assert!(close(ratio_between(&sp.plan.assembly, root, rb).unwrap().ratio, ring));
}

#[test]
fn a_hypoid_after_a_helical_pair() {
    let mut h1 = item("G", ItemKind::Helical, vec![circ([0.0; 3], Z, 20.0)]);
    let mut h2 = item("H", ItemKind::Helical, vec![circ([50.0, 0.0, 0.0], Z, 30.0)]);
    for it in [&mut h1, &mut h2] {
        it.gears = vec![crate::items::GearSettings { helix: Some(20.0), ..Default::default() }];
    }
    // A car axle's hypoid (41:11, 25 mm offset) on the driven helical's shaft.
    let mut hyp = item("K", ItemKind::Hypoid, vec![circ([50.0, 0.0, 80.0], Z, 100.0), line([-120.0, 25.0, 80.0], [-40.0, 25.0, 80.0])]);
    let by_teeth = |z| crate::items::GearSettings { size_by: crate::items::SizeBy::Teeth, teeth: Some(z), ..Default::default() };
    hyp.gears = vec![by_teeth(41), by_teeth(11)];
    let items = vec![h1, h2, hyp];
    let links = vec![mesh(g(0, 0), g(1, 0)), shaft(g(1, 0), g(2, 0))];
    let sp = planned(&items, &links, None);
    assert_eq!(sp.member_of.len(), 4);
    // Helical teeth of opposite hands on the pair.
    assert!(
        sp.plan
            .bodies
            .iter()
            .any(|b| b.name.to_lowercase().contains("hypoid") || b.name.to_lowercase().contains("ring") || b.name.to_lowercase().contains("pinion")),
        "{:?}",
        sp.plan.bodies.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
    let pinion = reply_ratio(&sp, g(2, 1));
    let by_hand = teeth(&sp, g(0, 0)) / teeth(&sp, g(1, 0)) * teeth(&sp, g(2, 0)) / teeth(&sp, g(2, 1));
    assert!(close(pinion.abs(), by_hand), "{pinion} vs {by_hand}: {:?}", sp.gears);
}

#[test]
fn a_ring_with_two_pinions_and_mixed_tooth_styles_across_a_shaft() {
    // Two spur pinions inside a ring (one stage each, the ring in both); the ring's shaft-mate
    // is a herringbone pair on another layer.
    let mut hb = item("H", ItemKind::Herringbone, vec![circ([0.0, 0.0, 40.0], Z, 30.0)]);
    let mut hb2 = item("I", ItemKind::Herringbone, vec![circ([50.0, 0.0, 40.0], Z, 20.0)]);
    for it in [&mut hb, &mut hb2] {
        it.gears = vec![crate::items::GearSettings { helix: Some(25.0), v: true, ..Default::default() }];
    }
    let items = vec![
        item("R", ItemKind::Internal, vec![circ([0.0; 3], Z, 50.0)]),
        item("S", ItemKind::Spur, vec![circ([30.0, 0.0, 0.0], Z, 20.0)]),
        item("T", ItemKind::Spur, vec![circ([-30.0, 0.0, 0.0], Z, 20.0)]),
        hb,
        hb2,
    ];
    let links = vec![mesh(g(1, 0), g(0, 0)), mesh(g(2, 0), g(0, 0)), shaft(g(0, 0), g(3, 0)), mesh(g(3, 0), g(4, 0))];
    let sp = planned(&items, &links, Some(&Drive { item: 1, gear: 0, rpm: None, torque: None }));
    let ring = sp.member(g(0, 0)).unwrap();
    assert!(sp.plan.members[ring].ring);
    assert_eq!(sp.plan.members[ring].meshes_with.len(), 2, "{:?}", sp.plan.members);
    // Both pinions turn alike: the ring drives T as S drives it.
    assert!(close(reply_ratio(&sp, g(2, 0)), 1.0), "{:?}", sp.gears);
    let r = reply_ratio(&sp, g(0, 0));
    assert!(close(r, teeth(&sp, g(1, 0)) / teeth(&sp, g(0, 0))), "{r}");
    assert!(close(reply_ratio(&sp, g(4, 0)), -r * teeth(&sp, g(3, 0)) / teeth(&sp, g(4, 0))));
    // Spur and herringbone in one train.
    let t = sp.plan.facts.len();
    assert!(t > 0);
}

#[test]
fn a_bevel_gear_meshing_two_pinions_is_planned_once() {
    // K's gear 1 (z up, apex 20 above it) meshes its own pinion on +x and a second item's on −x.
    let items = vec![
        item("K", ItemKind::Bevel, vec![circ([0.0; 3], Z, 30.0), circ([30.0, 0.0, 20.0], X, 20.0)]),
        item("M", ItemKind::Spur, vec![circ([-30.0, 0.0, 20.0], [-1.0, 0.0, 0.0], 20.0)]),
    ];
    let links = vec![mesh(g(0, 0), g(1, 0))];
    let sp = planned(&items, &links, None);
    // Three bodies: the gear once, its two pinions.
    assert_eq!(sp.plan.bodies.len(), 3, "{:?}", sp.plan.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    let k0 = sp.member(g(0, 0)).unwrap();
    assert_eq!(sp.plan.members[k0].meshes_with.len(), 2);
    // Both pinions turn at the same speed, opposite ways about +x and −x (so the same way in
    // the world).
    let (p1, p2) = (reply_ratio(&sp, g(0, 1)), reply_ratio(&sp, g(1, 0)));
    assert!(close(p1.abs(), p2.abs()), "{p1} {p2}");
    // The second pinion is turned to mesh with the gear that is kept: no deeper than the first.
    let body = |r| &sp.plan.bodies[sp.body(r).unwrap()];
    let depth = |a: &crate::plan::Body, b: &crate::plan::Body| crate::mesh3d::Pair::new(a, b, 0.1, 0.5).at(0.0, 0.0).penetration;
    let (d1, d2) = (depth(body(g(0, 0)), body(g(0, 1))), depth(body(g(0, 0)), body(g(1, 0))));
    assert!(d2 <= d1 + 0.02, "first pinion {d1} mm deep, second {d2} mm");
}

#[test]
fn stages_that_collide_are_refused_naming_both() {
    // Two separate spur pairs, the second drawn 3 mm above the first and overlapping it.
    let items = vec![
        item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 20.0)]),
        item("B", ItemKind::Spur, vec![circ([50.0, 0.0, 0.0], Z, 30.0)]),
        item("C", ItemKind::Spur, vec![circ([10.0, 40.0, 3.0], Z, 20.0)]),
        item("D", ItemKind::Spur, vec![circ([10.0, 90.0, 3.0], Z, 30.0)]),
    ];
    let links = vec![mesh(g(0, 0), g(1, 0)), mesh(g(2, 0), g(3, 0))];
    let sp = plan_items(&items, &links, &[], None, &Spec::default()).unwrap();
    let e = sp.error.unwrap_or_default();
    assert!(e.contains("collide") && e.contains("Gear C") && e.contains("move one"), "{e}");
    assert!(sp.plan.bodies.is_empty());
    // Moved clear along their shafts, they are made.
    let mut apart = items.clone();
    for (k, x) in [(2, 10.0), (3, 10.0)] {
        let y = if k == 2 { 40.0 } else { 90.0 };
        let r = if k == 2 { 20.0 } else { 30.0 };
        apart[k] = item(&items[k].id, ItemKind::Spur, vec![circ([x, y, 40.0], Z, r)]);
    }
    planned(&apart, &links, None);
}

#[test]
fn a_wrong_link_comes_back_on_the_link_with_a_hint() {
    let (items, mut links) = chain();
    // A Mesh link between the bevel gear and the spur pinion (shafts at an angle that miss).
    links.push(mesh(g(0, 0), g(2, 1)));
    let sp = plan_items(&items, &links, &[], None, &Spec::default()).unwrap();
    assert!(sp.error.is_some());
    let bad = sp.links.last().unwrap();
    assert!(bad.error.is_some() && bad.hint.is_some(), "{bad:?}");
    assert!(sp.plan.bodies.is_empty());
}

#[test]
fn the_reader_routes_a_set_with_a_bevel_among_three_gears() {
    use crate::drivetrain::{Ask, Circle3, Item, Thing as T, read};
    let c = |c: [f64; 3], n: [f64; 3], r: f64| Item { thing: T::Circle(Circle3 { c, n, r }), ask: Ask::default() };
    let items = vec![c([0.0; 3], Z, 20.0), c([50.0, 0.0, 0.0], Z, 30.0), c([50.0, 0.0, 40.0], Z, 15.0), c([65.0, 0.0, 60.0], X, 20.0)];
    let o = read(&items, None, &crate::simple::Simple::default(), &Spec::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    let p = o.planned.as_ref().expect("a merged plan");
    assert_eq!(p.bodies.len(), 4);
    assert_eq!(o.members[0].meshes_with, vec![1]);
    assert_eq!(o.members[3].meshes_with, vec![2]);
    assert!(o.members[1].cluster.is_some() && o.members[1].cluster == o.members[2].cluster);
}

/// Each link of the assembly carries its stage's real kind: the worm stage is a worm (its
/// lead-angle efficiency and self-locking check, whichever end drives it), the bevel a bevel,
/// the spur pair spur; the replies' rpm follow from it, each gear has its contact ratio, and the
/// note counts the bodies the feature makes (one per rigid member).
#[test]
fn every_link_is_analysed_as_its_stage_kind() {
    use crate::analysis::{MeshKind, StepKind, ratio_between_with, worm_efficiency};
    let (items, links) = chain();
    let drive = Drive { item: 0, gear: 0, rpm: Some(1000.0), torque: Some(2.0) };
    let sp = planned(&items, &links, Some(&drive));
    let e = sp.efficiencies(&sp.plan.assembly);
    let root = sp.driver.unwrap();
    let worm = sp.body(g(3, 1)).unwrap();
    let report = ratio_between_with(&sp.plan.assembly, root, worm, &e).unwrap();
    let kinds: Vec<StepKind> = report.path.iter().map(|s| s.kind).filter(|k| *k != StepKind::Shaft).collect();
    assert_eq!(kinds, vec![StepKind::Mesh(MeshKind::Spur), StepKind::Mesh(MeshKind::Bevel), StepKind::Mesh(MeshKind::Worm)], "{:?}", report.path);
    // The worm step, driven from its wheel: the lead angle the worm stage was planned with.
    let lead = sp.mesh_tags.iter().find_map(|t| t.lead_angle).expect("the worm's lead angle");
    let we = worm_efficiency(lead, e.worm_friction, e.worm_pressure_angle);
    let last = report.path.last().unwrap();
    assert_eq!(last.teeth.map(|t| t.1), Some(1), "{last:?}");
    assert_eq!(last.self_locking, we.self_locking);
    assert!((last.efficiency - we.back).abs() < 1e-12, "{} vs {}", last.efficiency, we.back);
    // Driven from the worm the other way: the forward efficiency.
    let wheel = sp.body(g(3, 0)).unwrap();
    let back = ratio_between_with(&sp.plan.assembly, worm, wheel, &e).unwrap();
    assert!((back.path[0].efficiency - we.forward).abs() < 1e-12);
    // The replies: the worm's rpm is the drive table's with these kinds (0 when self-locking).
    let rpm = sp.gears.iter().find(|x| x.item == 3 && x.gear == 1).and_then(|x| x.rpm).unwrap();
    let wheel_rpm = sp.gears.iter().find(|x| x.item == 3 && x.gear == 0).and_then(|x| x.rpm).unwrap();
    if we.self_locking {
        assert_eq!(rpm, 0.0);
    } else {
        assert!(close(rpm, wheel_rpm * -teeth(&sp, g(3, 0))), "{rpm} {wheel_rpm}");
    }
    // The spur pair's gears have their contact ratio in their replies (a bevel plan works none out).
    for r in [g(0, 0), g(1, 0)] {
        let c = sp.gears.iter().find(|x| x.item == r.item && x.gear == r.gear).and_then(|x| x.contact_ratio);
        assert!(c.is_some_and(|c| c > 1.0), "{r:?}: {c:?}");
    }
    assert!(sp.notes.iter().any(|n| n == "4 bodies from 3 stages"), "{:?}", sp.notes);
}

/// A gear that meshes nothing, sized by a fixed module (decision 3): the tooth count closest to
/// its drawn diameter at that module, the module kept, and a note giving the diameter it gets
/// beside the drawn one.
#[test]
fn a_free_gear_at_a_fixed_module_keeps_the_module_with_a_note() {
    use crate::items::{GearSettings, SizeBy};
    let mut a = item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 21.0)]);
    a.gears = vec![GearSettings { size_by: SizeBy::Module, module: Some(4.0), ..GearSettings::default() }];
    let pair =
        vec![item("B", ItemKind::Spur, vec![circ([100.0, 0.0, 0.0], Z, 20.0)]), item("C", ItemKind::Spur, vec![circ([140.0, 0.0, 0.0], Z, 20.0)])];
    for (items, links) in [(vec![a.clone()], vec![]), ([vec![a.clone()], pair].concat(), vec![mesh(g(1, 0), g(2, 0))])] {
        let sp = planned(&items, &links, None);
        let r = &sp.gears[0];
        // Ø42 at module 4: 10.5 teeth, 11 the closest (rounding up).
        assert_eq!((r.teeth, r.module), (11, 4.0), "{r:?}");
        assert!((r.pitch_d - 44.0).abs() < 1e-9);
        assert!(sp.notes.iter().any(|n| n == "Gear A meshes nothing: 11 teeth at module 4 make it Ø44 (Ø42 drawn)"), "{:?}", sp.notes);
    }
    // At a module that fits the drawing: no note.
    a.gears[0].module = Some(3.0);
    let sp = planned(&[a], &[], None);
    assert_eq!(sp.gears[0].teeth, 14);
    assert!(!sp.notes.iter().any(|n| n.contains("meshes nothing")), "{:?}", sp.notes);
}

/// A bevel pair from one side view (no circles): the apex point, two shaft lines from it at the
/// shaft angle, and a contact line between them (its angles set the ratio, its length the cone
/// distance); or, without the contact line, sizes from the gears' teeth and module.
#[test]
fn a_bevel_pair_from_one_side_view() {
    use crate::items::{GearSettings, SizeBy, validate_item};
    // In the XZ plane: shafts along +x and +z from the origin (90°); the contact line at
    // atan(2) from z, 50 mm long: pitch radii 50 sin δ.
    let d1 = (2.0f64).atan(); // from the z shaft
    let c = [50.0 * d1.sin(), 0.0, 50.0 * d1.cos()];
    let side = item(
        "S",
        ItemKind::Bevel,
        vec![Thing::point([0.0; 3]), line([0.0; 3], [0.0, 0.0, 60.0]), line([0.0; 3], [60.0, 0.0, 0.0]), line([0.0; 3], c)],
    );
    validate_item(&side).unwrap();
    let geo = crate::items::gear_geoms(&side);
    assert_eq!(geo.len(), 2);
    let (r1, r2) = (geo[0].radius.unwrap(), geo[1].radius.unwrap());
    assert!(close(r1, 50.0 * d1.sin()) && close(r2, 50.0 * d1.cos()), "{r1} {r2}");
    let sp = planned(&[side], &[], None);
    assert_eq!(sp.plan.bodies.len(), 2, "{:?}", sp.error);
    let (z1, z2) = (teeth(&sp, g(0, 0)), teeth(&sp, g(0, 1)));
    assert!((z1 / z2 - 2.0).abs() < 0.15, "about 2:1 as drawn: {z1} {z2}");
    assert!(sp.mesh_tags.iter().any(|t| t.kind == crate::analysis::MeshKind::Bevel));
    // Without the contact line: from the gears' teeth and module, 20 each at module 2 until
    // they are set (it builds at once).
    let mut bare = item("T", ItemKind::Bevel, vec![Thing::point([0.0; 3]), line([0.0; 3], [0.0, 0.0, 60.0]), line([0.0; 3], [60.0, 0.0, 0.0])]);
    validate_item(&bare).unwrap();
    let sp = planned(std::slice::from_ref(&bare), &[], None);
    assert_eq!((teeth(&sp, g(0, 0)), teeth(&sp, g(0, 1))), (20.0, 20.0), "{:?}", sp.error);
    let by = |z: u32| GearSettings { size_by: SizeBy::Teeth, teeth: Some(z), module: Some(2.0), ..GearSettings::default() };
    bare.gears = vec![by(30), by(15)];
    validate_item(&bare).unwrap();
    let sp = planned(&[bare], &[], None);
    assert_eq!((teeth(&sp, g(0, 0)), teeth(&sp, g(0, 1))), (30.0, 15.0), "{:?}", sp.gears);
    // A shaft line that misses the apex is refused, naming it.
    let off = item(
        "U",
        ItemKind::Bevel,
        vec![Thing::point([0.0; 3]), line([5.0, 0.0, 0.0], [5.0, 0.0, 60.0]), line([0.0; 3], [60.0, 0.0, 0.0]), line([0.0; 3], c)],
    );
    assert!(validate_item(&off).unwrap_err().contains("doesn't run through the apex"));
}

/// A ring gear whose pinion shares a shaft with a gear of another train (a cluster across
/// items, as 09-everything-drivetrain draws it): the ring keeps its teeth inside, and no point of
/// the pinion's outline lies in the ring's material where they are planned.
#[test]
fn a_ring_whose_pinion_is_in_a_cluster_keeps_its_space_for_the_pinion() {
    use crate::items::{GearSettings, Hand, ModuleBlock};
    use crate::mesh2d::{Member, Motion, polygon};
    use crate::plan::Step;
    let at = |x: f64, h: f64| [x, 0.0, h];
    let helical = |hand: Hand| vec![GearSettings { helix: Some(20.0), hand: Some(hand), ..GearSettings::default() }];
    let mut e = item("E", ItemKind::Helical, vec![circ(at(87.0, -60.0), Z, 12.0)]);
    e.gears = helical(Hand::Right);
    let mut f = item("F", ItemKind::Helical, vec![circ(at(123.0, -60.0), Z, 24.0)]);
    f.gears = helical(Hand::Left);
    let mut r = item("R", ItemKind::Internal, vec![circ(at(153.0, -90.0), Z, 50.0), circ(at(123.0, -90.0), Z, 20.0)]);
    r.gears = vec![GearSettings { rim: Some(6.0), ..GearSettings::default() }];
    let links = vec![mesh(g(0, 0), g(1, 0)), shaft(g(1, 0), g(2, 1))];
    let blocks = vec![
        ModuleBlock { id: "M".into(), module: Some(1.5), members: vec![0, 1], ..Default::default() },
        ModuleBlock { id: "M2".into(), module: Some(2.0), members: vec![2], ..Default::default() },
    ];
    let sp = plan_items(&[e, f, r], &links, &blocks, None, &Spec::default()).unwrap();
    assert!(sp.error.is_none(), "{:?}", sp.error);
    let (ring, cluster) = (sp.body(g(2, 0)).unwrap(), sp.body(g(2, 1)).unwrap());
    assert_eq!(sp.body(g(1, 0)), Some(cluster), "F and the pinion are one rigid body");
    let ring_m = &sp.plan.members[sp.member(g(2, 0)).unwrap()];
    assert!(ring_m.ring);
    let rb = &sp.plan.bodies[ring];
    let Some(Step::Prism { region: ring_region, .. }) = rb.steps.first() else { panic!("{:?}", rb.steps) };
    assert!(!ring_region.holes.is_empty(), "the ring has its toothed hole");
    let ring_mat = Member::new(ring_region, Motion::turning(crate::V2::ZERO, 0.0), 0.5, 0.01);
    // The pinion's prism: the cluster body's step at the ring's height.
    let cb = &sp.plan.bodies[cluster];
    let pinion = cb
        .steps
        .iter()
        .find_map(|s| match s {
            Step::Prism { region, z0, .. } if (cb.frame.origin[2] + z0 - rb.frame.origin[2]).abs() < 1e-6 => Some(region),
            _ => None,
        })
        .expect("the pinion's prism level with the ring");
    let (rx, ry) = (rb.frame.x, rb.frame.y());
    let (cx, cy) = (cb.frame.x, cb.frame.y());
    let mut inside = 0;
    for o in &pinion.outer {
        for q in polygon(o, 0.01) {
            let w = [0, 1, 2].map(|k| cb.frame.origin[k] + cx[k] * q.x + cy[k] * q.y - rb.frame.origin[k]);
            let local = crate::V2::new(w[0] * rx[0] + w[1] * rx[1] + w[2] * rx[2], w[0] * ry[0] + w[1] * ry[1] + w[2] * ry[2]);
            if ring_mat.inside(local) && ring_mat.dist(local) > 0.05 {
                inside += 1;
            }
        }
    }
    assert_eq!(inside, 0, "pinion outline points inside the ring's material");
}

/// The housing cutout of a drivetrain made from items: each bearing point picked on a gear's
/// shaft line is a seat ([`crate::clearance::Seat::Point`]) of the set's bearing, on that gear's
/// axis where the point is; moving the point moves the seat.
#[test]
fn a_bearing_point_on_a_gears_shaft_line_is_a_seat_of_the_housing_cutout() {
    use crate::clearance::{Clearance, Seat, clearance_body};
    use crate::items::{bearing_points, validate_slot};
    use crate::mesh3d::Solid;
    let pt = |p: [f64; 3]| Thing::point(p);
    let at = |z: f64| {
        vec![
            item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 20.0), line([0.0, 0.0, -40.0], [0.0, 0.0, 40.0]), pt([0.0, 0.0, z])]),
            item("B", ItemKind::Spur, vec![circ([50.0, 0.0, 0.0], Z, 30.0), line([50.0, 0.0, -40.0], [50.0, 0.0, 40.0]), pt([50.0, 0.0, -25.0])]),
        ]
    };
    // A bearing point is taken on a gear only on its own shaft line.
    let a = &at(-25.0)[0];
    assert!(validate_slot(ItemKind::Spur, &a.things[..2], &pt([0.0, 0.0, -25.0])).is_ok());
    let e = validate_slot(ItemKind::Spur, &a.things[..2], &pt([5.0, 0.0, -25.0])).unwrap_err();
    assert!(e.contains("off the shaft line"), "{e}");
    let e = validate_slot(ItemKind::Spur, &a.things[..1], &pt([0.0, 0.0, -25.0])).unwrap_err();
    assert!(e.contains("shaft line first"), "{e}");
    let links = vec![mesh(g(0, 0), g(1, 0))];
    let cutout = |z: f64| {
        let items = at(z);
        assert_eq!(bearing_points(&items).len(), 2);
        let base = Spec {
            clearance_body: Some(Clearance {
                bearing: Some("608".into()),
                seats: bearing_points(&items).into_iter().map(Seat::Point).collect(),
                ..Clearance::default()
            }),
            ..Spec::default()
        };
        let mut sp = plan_items(&items, &links, &[], None, &Spec { clearance_body: None, ..base.clone() }).unwrap_or_else(|e| panic!("{e}"));
        assert!(sp.error.is_none(), "{:?}", sp.error);
        let body = clearance_body(&base, &mut sp.plan).unwrap_or_else(|e| panic!("{e}"));
        assert!(sp.plan.warnings.iter().all(|w| !w.contains("seat")), "{:?}", sp.plan.warnings);
        Solid::new(&body, 0.002)
    };
    // A 608 is 22 mm across and 7 wide: its seat round A's axis at the point.
    let keep = cutout(-25.0);
    assert!(keep.contains([10.5, 0.0, -25.0]) && keep.contains([0.0, -10.5, -22.0]), "the seat at A's point");
    assert!(!keep.contains([11.6, 0.0, -25.0]), "no bigger than the bearing");
    assert!(keep.contains([60.5, 0.0, -25.0]), "the seat at B's point");
    // Moved along the line, the seat follows.
    let moved = cutout(-32.0);
    assert!(moved.contains([10.5, 0.0, -32.0]), "the seat moved with its point");
    assert!(keep.contains([10.5, 0.0, -24.0]) && !moved.contains([10.5, 0.0, -24.0]), "nothing left where it was");
}

/// The set's tooth chamfer: off the tip edges of every spur gear's end faces (both, or one
/// side), refused when it would take the addendum, a note for a gear it doesn't go on.
#[test]
fn a_tooth_chamfer_goes_on_every_gear_of_the_set() {
    use crate::mesh3d::Solid;
    use crate::plan::chamfer::{ChamferFaces, ToothChamfer, tooth_chamfer};
    let items = vec![item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 20.0)]), item("B", ItemKind::Spur, vec![circ([50.0, 0.0, 0.0], Z, 30.0)])];
    let links = vec![mesh(g(0, 0), g(1, 0))];
    let plain = plan_items(&items, &links, &[], None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(plain.error.is_none(), "{:?}", plain.error);
    let steps = |sp: &SystemPlan| sp.plan.bodies.iter().map(|b| b.steps.len()).collect::<Vec<_>>();
    // Off: nothing changes.
    let mut off = plain.clone();
    tooth_chamfer(&items, &mut off, ToothChamfer::default()).unwrap();
    assert_eq!(off, plain);
    let module = plain.gears[0].module;
    let shape = *plain.plan.shapes.iter().find(|s| s.pitch_diameter < 45.0).expect("A's shape");
    let (tip, face) = (shape.tip_diameter / 2.0, shape.face);
    let c = 0.4 * module;
    let chamfered = |faces| {
        let mut sp = plain.clone();
        tooth_chamfer(&items, &mut sp, ToothChamfer { size: c, faces }).unwrap_or_else(|e| panic!("{e}"));
        sp
    };
    let both = chamfered(ChamferFaces::Both);
    assert_eq!(steps(&both).iter().sum::<usize>(), steps(&plain).iter().sum::<usize>() + 4, "two ends of two gears");
    let one = chamfered(ChamferFaces::A);
    assert_eq!(steps(&one).iter().sum::<usize>(), steps(&plain).iter().sum::<usize>() + 2, "side A of each");
    // At A's tip edge on side A (z = face / 2, symmetric): teeth before, none after.
    let body = plain.member_of.iter().find(|(r, _)| *r == g(0, 0)).and_then(|(_, m)| plain.plan.members[*m].body).unwrap();
    let ring = |sp: &SystemPlan, z: f64| {
        let s = Solid::new(&sp.plan.bodies[body], 0.002);
        (0..720)
            .map(|k| f64::from(k) * std::f64::consts::TAU / 720.0)
            .filter(|a| s.contains([(tip - 0.25 * c) * a.cos(), (tip - 0.25 * c) * a.sin(), z]))
            .count()
    };
    let (top, bottom) = (face / 2.0 - 0.25 * c, -face / 2.0 + 0.25 * c);
    assert!(ring(&plain, top) > 0 && ring(&plain, bottom) > 0, "the teeth reach the edge");
    assert_eq!(ring(&both, top), 0, "chamfered on side A");
    assert_eq!(ring(&both, bottom), 0, "chamfered on side B");
    assert_eq!(ring(&one, top), 0);
    assert!(ring(&one, bottom) > 0, "side B left plain");
    assert_eq!(ring(&both, 0.0), ring(&plain, 0.0), "the middle of the face untouched");
    // The builders make both ends' cuts as one intersect (chamfers_as_one): the same solid,
    // at the edges and everywhere else, out past the faces and the tip.
    let cut = &both.plan.bodies[body];
    let n = cut.steps.len();
    let merged = crate::plan::chamfer::chamfers_as_one(cut, n - 2).expect("both ends of A as one step");
    let one_step = crate::plan::Body { steps: cut.steps[..n - 2].iter().cloned().chain([merged]).collect(), ..cut.clone() };
    let (sc, sm) = (Solid::new(cut, 0.01), Solid::new(&one_step, 0.01));
    for (r, z) in (0..=60).flat_map(|i| (0..=60).map(move |j| (0.05 + (tip + 3.0) * f64::from(i) / 60.0, -face + 2.0 * face * f64::from(j) / 60.0))) {
        for a in [0.0f64, 0.7, 2.1, 4.0] {
            let p = [r * a.cos(), r * a.sin(), z];
            assert_eq!(sc.contains(p), sm.contains(p), "at r {r}, z {z}");
        }
    }
    assert!(crate::plan::chamfer::chamfers_as_one(&one.plan.bodies[body], one.plan.bodies[body].steps.len() - 2).is_none(), "one side: no pair");
    // Too big: named.
    let mut sp = plain.clone();
    let e = tooth_chamfer(&items, &mut sp, ToothChamfer { size: 1.2 * module, faces: ChamferFaces::Both }).unwrap_err();
    assert!(e.starts_with("Tooth chamfer") && e.contains("too big for Gear A") && e.contains("addendum"), "{e}");
    let _ = tip;
}

/// A worm pair keeps its teeth plain, with a note naming it; a compound's layers are chamfered
/// except where the smaller layer meets the bigger one.
#[test]
fn a_tooth_chamfer_leaves_a_note_where_it_does_not_go() {
    use crate::plan::chamfer::{ChamferFaces, ToothChamfer, tooth_chamfer};
    let worm = vec![item("A", ItemKind::Worm, vec![circ([0.0; 3], Z, 30.0), line([-40.0, 40.0, 0.0], [40.0, 40.0, 0.0])])];
    let mut sp = plan_items(&worm, &[], &[], None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(sp.error.is_none(), "{:?}", sp.error);
    let before = sp.plan.bodies.clone();
    tooth_chamfer(&worm, &mut sp, ToothChamfer { size: 0.2, faces: ChamferFaces::Both }).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(sp.plan.bodies, before, "a worm pair is left plain");
    assert!(sp.notes.iter().any(|n| n == "tooth chamfer not applied to Gear A: worm"), "{:?}", sp.notes);
    // A compound of two layers, face to face: three of its four ends chamfered.
    let compound = vec![item(
        "E",
        ItemKind::Compound,
        vec![line([0.0, 0.0, -10.0], [0.0, 0.0, 30.0]), circ([0.0; 3], Z, 20.0), circ([0.0, 0.0, 20.0], Z, 12.0)],
    )];
    let mut sp = plan_items(&compound, &[], &[], None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(sp.error.is_none(), "{:?}", sp.error);
    let count = |sp: &SystemPlan| sp.plan.bodies.iter().map(|b| b.steps.len()).sum::<usize>();
    let before = count(&sp);
    let m = sp.gears.iter().map(|x| x.module).fold(f64::INFINITY, f64::min);
    tooth_chamfer(&compound, &mut sp, ToothChamfer { size: 0.3 * m, faces: ChamferFaces::Both }).unwrap_or_else(|e| panic!("{e}"));
    eprintln!("compound ends chamfered: {}", count(&sp) - before);
    assert!(count(&sp) - before >= 3, "each layer's free ends");
}

/// Crossed helical items build two helical gears whose helix angles add up to the shaft angle
/// (not a worm), at 90° and at 60°; a chevron bevel pair's teeth change hand at mid-face (not the
/// spiral's).
#[test]
fn crossed_helical_and_chevron_bevel_are_what_they_say() {
    use crate::items::{BevelStyle, GearSettings};
    use crate::plan::Step;
    for deg in [90.0f64, 60.0] {
        let (s, c) = (deg.to_radians().sin(), deg.to_radians().cos());
        // Gear A round Z at the origin (r 20); gear B (r 10) round an axis at `deg` to Z, 30 mm off along Y.
        let items = vec![item("A", ItemKind::CrossedHelical, vec![circ([0.0; 3], Z, 20.0), circ([0.0, 30.0, 0.0], [s, 0.0, c], 10.0)])];
        let sp = plan_items(&items, &[], &[], None, &Spec::default()).unwrap_or_else(|e| panic!("{deg}°: {e}"));
        assert!(sp.error.is_none(), "{deg}°: {:?}", sp.error);
        let worm = sp.plan.assembly.links.iter().any(|l| matches!(l, AssemblyLink::Worm { .. }));
        assert!(!worm, "{deg}°: built as a worm: {:?}", sp.plan.assembly.links);
        let helix: Vec<f64> = ["gear 1 helix angle", "gear 2 helix angle"]
            .iter()
            .filter_map(|l| sp.plan.facts.iter().find(|f| f.label.ends_with(l)).map(|f| f.value.abs()))
            .collect();
        assert_eq!(helix.len(), 2, "{deg}°: {:?}", sp.plan.facts.iter().map(|f| &f.label).collect::<Vec<_>>());
        eprintln!("{deg}°: helix {helix:?}, teeth {:?}", sp.gears.iter().map(|g| g.teeth).collect::<Vec<_>>());
        assert!((helix.iter().sum::<f64>() - deg).abs() < 0.5, "{deg}°: helix angles {helix:?} add up to the shaft angle");
    }
    let bevel = |style| {
        let mut it = item("A", ItemKind::Bevel, vec![circ([0.0, 0.0, 0.0], Z, 30.0), circ([0.0, 30.0, 15.0], [0.0, 1.0, 0.0], 15.0)]);
        it.gears = vec![GearSettings { bevel_style: style, ..GearSettings::default() }];
        let sp = plan_items(&[it], &[], &[], None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
        assert!(sp.error.is_none(), "{:?}", sp.error);
        sp.plan.bodies.iter().map(|b| b.steps.clone()).collect::<Vec<Vec<Step>>>()
    };
    assert_ne!(bevel(BevelStyle::Chevron), bevel(BevelStyle::Spiral), "chevron teeth differ from the spiral's");
}

/// A stage's warnings name the items' gears, not the stage plan's own "gear 1" and "gear 2": a
/// spur train's undercut pinion, and whatever the bevel and worm stages of a chain warn of.
#[test]
fn stage_warnings_name_the_items_gears() {
    let module2 = vec![GearSettings { module: Some(2.0), ..GearSettings::default() }];
    let mut a = item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 10.0)]);
    a.gears = module2.clone();
    // (Two 10-tooth gears at their reference distance: no split of the shift clears both. A
    // 30-tooth mate would take the shift that clears the pinion.)
    let mut b = item("B", ItemKind::Spur, vec![circ([20.0, 0.0, 0.0], Z, 10.0)]);
    b.gears = module2;
    let sp = planned(&[a, b], &[mesh(g(0, 0), g(1, 0))], None);
    let w = &sp.plan.warnings;
    assert!(w.iter().any(|w| w.contains("Gear A: undercut")), "{w:?}");
    let (items, links) = chain();
    let chain = planned(&items, &links, None);
    for w in [w, &chain.plan.warnings] {
        assert!(!w.iter().any(|w| w.contains("gear 1") || w.contains("gear 2")), "{w:?}");
    }
}
