//! The items contract: slot refusals per type, compounds from parallel sketches, module blocks,
//! face sides, suggestions, the graph split into stages, and the mapping onto the drivetrain
//! reader.

use crate::items::*;
use crate::plan::{Kind, Spec};
use crate::simple::Simple;

const X: V = [1.0, 0.0, 0.0];
const Y: V = [0.0, 1.0, 0.0];
const Z: V = [0.0, 0.0, 1.0];

fn circ(c: V, n: V, r: f64) -> Thing {
    Thing::circle(c, n, r)
}
fn line(a: V, b: V) -> Thing {
    Thing::line(a, b)
}

/// Picks `things` one by one into a new item of `kind`, every one accepted.
fn picked(kind: ItemKind, things: &[Thing]) -> Vec<SlotIndex> {
    let mut have: Vec<Thing> = Vec::new();
    let mut out = Vec::new();
    for t in things {
        match validate_slot(kind, &have, t) {
            Ok(s) => out.push(s),
            Err(e) => panic!("{kind:?}: pick {} refused: {e}", have.len() + 1),
        }
        have.push(t.clone());
    }
    out
}

/// The refusal of `thing` after `existing`, which must contain `word`.
fn refused(kind: ItemKind, existing: &[Thing], thing: Thing, word: &str) {
    picked(kind, existing);
    match validate_slot(kind, existing, &thing) {
        Ok(s) => panic!("{kind:?}: {thing:?} accepted as {s:?}, wanted a refusal about {word:?}"),
        Err(e) => assert!(e.contains(word), "{kind:?}: refusal {e:?} doesn't say {word:?}"),
    }
}

fn item(id: &str, kind: ItemKind, things: Vec<Thing>) -> GearItem {
    let it = GearItem::new(id, kind).with(things);
    validate_item(&it).unwrap_or_else(|e| panic!("{e}"));
    it
}

fn mesh(a: (usize, usize), b: (usize, usize)) -> ItemLink {
    ItemLink { a: GearRef::new(a.0, a.1), b: GearRef::new(b.0, b.1), kind: LinkKind::Mesh }
}
fn shaft(a: (usize, usize), b: (usize, usize)) -> ItemLink {
    ItemLink { a: GearRef::new(a.0, a.1), b: GearRef::new(b.0, b.1), kind: LinkKind::Shaft }
}

fn close(a: V, b: V) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
}

// ---------------------------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------------------------

#[test]
fn every_kind_names_and_parses() {
    for k in ItemKind::ALL.iter().copied() {
        assert_eq!(ItemKind::parse(k.name()), Some(k));
        assert!(!slots(k).is_empty());
    }
    assert_eq!(ItemKind::parse("spiral_bevel"), Some(ItemKind::Bevel));
    assert_eq!(ItemKind::parse("nonsense"), None);
}

#[test]
fn spur_takes_a_circle_and_its_own_shaft_line() {
    let c = circ([0.0; 3], Z, 20.0);
    let s = picked(ItemKind::Spur, &[c.clone(), line([0.0, 0.0, -5.0], [0.0, 0.0, 30.0])]);
    assert_eq!(s[0].name, "pitch");
    assert_eq!(s[1].name, "axis");
    assert_eq!(next_slot(ItemKind::Spur, &[]), Some("pitch circle"));
    assert_eq!(next_slot(ItemKind::Spur, std::slice::from_ref(&c)), None);
    refused(ItemKind::Spur, std::slice::from_ref(&c), line([0.0; 3], [10.0, 0.0, 30.0]), "tilted");
    refused(ItemKind::Spur, std::slice::from_ref(&c), line([5.0, 0.0, 0.0], [5.0, 0.0, 30.0]), "off the shaft line");
    refused(ItemKind::Spur, std::slice::from_ref(&c), circ([50.0, 0.0, 0.0], Z, 10.0), "no room");
    // A bearing point goes on the gear's own shaft line: picked first, then the point on it.
    refused(ItemKind::Spur, std::slice::from_ref(&c), Thing::point([0.0; 3]), "shaft line first");
    let shafted = [c.clone(), line([0.0, 0.0, -5.0], [0.0, 0.0, 30.0])];
    assert_eq!(picked(ItemKind::Spur, &[shafted[0].clone(), shafted[1].clone(), Thing::point([0.0, 0.0, 20.0])])[2].name, "bearing");
    refused(ItemKind::Spur, &shafted, Thing::point([3.0, 0.0, 20.0]), "off the shaft line");
    refused(ItemKind::Spur, std::slice::from_ref(&c), c.clone(), "already picked");
    refused(ItemKind::Spur, &[], circ([0.0; 3], Z, 0.0), "no size");
    let construction = Thing::Line { a: [0.0; 3], b: [0.0, 0.0, 9.0], construction: true, ids: ThingIds::default() };
    refused(ItemKind::Spur, std::slice::from_ref(&c), construction, "construction");
}

#[test]
fn compound_from_two_parallel_sketches() {
    // A shaft line along z; circles in the sketch planes z = 0 and z = 12, one each.
    let l = line([30.0, 0.0, -5.0], [30.0, 0.0, 25.0]);
    let c1 = circ([30.0, 0.0, 0.0], Z, 20.0).with_ids(Some(1), "c1");
    let c2 = circ([30.0, 0.0, 12.0], Z, 10.0).with_ids(Some(2), "c1");
    let s = picked(ItemKind::Compound, &[l.clone(), c1.clone(), c2.clone()]);
    assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["axis", "pitch", "pitch2"]);
    // A third circle further along repeats the slot.
    let c3 = circ([30.0, 0.0, 20.0], [0.0, 0.0, -1.0], 8.0);
    assert_eq!(picked(ItemKind::Compound, &[l.clone(), c1.clone(), c2.clone(), c3])[3].name, "pitch3");
    // Circles first, line last.
    picked(ItemKind::Compound, &[c1.clone(), c2.clone(), l.clone()]);
    assert_eq!(next_slot(ItemKind::Compound, &[l.clone(), c1.clone()]), Some("pitch circle"));
    // Refusals: off the line, tilted, two in one plane.
    refused(ItemKind::Compound, &[l.clone(), c1.clone()], circ([31.0, 0.0, 12.0], Z, 10.0), "off the shaft line");
    refused(ItemKind::Compound, &[l.clone(), c1.clone()], circ([30.0, 0.0, 12.0], [0.0, 0.2, 1.0], 10.0), "tilted");
    refused(ItemKind::Compound, &[l.clone(), c1.clone()], circ([30.0, 0.0, 0.0], Z, 10.0), "same place");
    refused(ItemKind::Compound, std::slice::from_ref(&c1), circ([35.0, 0.0, 12.0], Z, 10.0), "off the first circle's axis");
    // One body: a shaft cluster of both gears; gears' side A along the line.
    let it = item("A", ItemKind::Compound, vec![l, c1, c2]);
    assert_eq!(it.gear_count(), 2);
    let g = gear_geoms(&it);
    assert!(close(g[0].axis, Z) && close(g[1].axis, Z));
    let st = stages(std::slice::from_ref(&it), &[]);
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].kind, StageKind::Shaft);
    assert_eq!(st[0].gears, vec![GearRef::new(0, 0), GearRef::new(0, 1)]);
    assert_eq!(st[0].from, StageFrom::Cluster { links: vec![], items: vec![0] });
}

#[test]
fn side_a_follows_a_picked_shaft_line() {
    // The circle's normal is -z but its shaft line runs +z from its first point: side A is +z.
    let it = item("A", ItemKind::Spur, vec![circ([0.0; 3], [0.0, 0.0, -1.0], 10.0), line([0.0, 0.0, -5.0], [0.0, 0.0, 5.0])]);
    assert!(close(gear_geoms(&it)[0].axis, Z));
    let bare = item("B", ItemKind::Spur, vec![circ([0.0; 3], [0.0, 0.0, -1.0], 10.0)]);
    assert!(close(gear_geoms(&bare)[0].axis, [0.0, 0.0, -1.0]));
}

#[test]
fn ring_gear_and_its_pinion_inside() {
    let ring = circ([0.0; 3], Z, 50.0);
    let s = picked(ItemKind::Internal, &[ring.clone(), circ([30.0, 0.0, 0.0], Z, 20.0)]);
    assert_eq!((s[0].name.as_str(), s[1].name.as_str()), ("pitch", "pitch2"));
    // Pinion first: the larger circle is still the ring.
    let both = [circ([30.0, 0.0, 0.0], Z, 20.0), ring.clone()];
    picked(ItemKind::Internal, &both);
    let s = assign(ItemKind::Internal, &both).unwrap();
    assert_eq!((s[0].name.as_str(), s[1].name.as_str()), ("pitch2", "pitch"));
    let it = item("A", ItemKind::Internal, vec![circ([30.0, 0.0, 0.0], Z, 20.0), ring.clone()]);
    let g = gear_geoms(&it);
    assert!(g[0].ring && g[0].radius == Some(50.0) && !g[1].ring);
    refused(ItemKind::Internal, std::slice::from_ref(&ring), circ([20.0, 0.0, 0.0], Z, 20.0), "doesn't touch");
    refused(ItemKind::Internal, std::slice::from_ref(&ring), circ([45.0, 0.0, 0.0], Z, 20.0), "not inside");
    refused(ItemKind::Internal, std::slice::from_ref(&ring), circ([30.0, 0.0, 4.0], Z, 20.0), "off the ring's plane");
    refused(ItemKind::Internal, std::slice::from_ref(&ring), circ([30.0, 0.0, 0.0], X, 20.0), "not parallel");
    let st = stages(std::slice::from_ref(&it), &[]);
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].kind, StageKind::Internal);
}

#[test]
fn rack_line_tangent_to_its_pinion() {
    let rack = line([-50.0, -20.0, 0.0], [50.0, -20.0, 0.0]);
    let s = picked(ItemKind::Rack, &[rack.clone(), circ([0.0; 3], Z, 20.0)]);
    assert_eq!((s[0].name.as_str(), s[1].name.as_str()), ("axis", "pitch"));
    refused(ItemKind::Rack, std::slice::from_ref(&rack), circ([0.0; 3], Z, 15.0), "doesn't touch");
    refused(ItemKind::Rack, std::slice::from_ref(&rack), circ([0.0, 0.0, 3.0], Z, 20.0), "off the pinion's plane");
    refused(ItemKind::Rack, std::slice::from_ref(&rack), circ([0.0; 3], X, 20.0), "not in the pinion's plane");
    // The pinion may come first; a second line has no room.
    picked(ItemKind::Rack, &[circ([0.0; 3], Z, 20.0), rack.clone()]);
    refused(ItemKind::Rack, std::slice::from_ref(&rack), line([0.0, 30.0, 0.0], [9.0, 30.0, 0.0]), "no room");
}

#[test]
fn planetary_sun_ring_and_planets() {
    let (sun, ring) = (circ([0.0; 3], Z, 10.0), circ([0.0; 3], Z, 30.0));
    let all = [ring.clone(), sun.clone(), circ([20.0, 0.0, 0.0], Z, 10.0), circ([-20.0, 0.0, 0.0], Z, 10.0)];
    picked(ItemKind::Planetary, &all);
    let s = assign(ItemKind::Planetary, &all).unwrap();
    assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["ring", "sun", "planet", "planet2"]);
    refused(ItemKind::Planetary, std::slice::from_ref(&sun), circ([2.0, 0.0, 0.0], Z, 30.0), "not concentric");
    refused(ItemKind::Planetary, std::slice::from_ref(&sun), circ([0.01, 0.0, 0.0], Z, 10.0), "same size");
    refused(ItemKind::Planetary, &[sun.clone(), ring.clone()], circ([25.0, 0.0, 0.0], Z, 10.0), "doesn't touch");
    refused(ItemKind::Planetary, &[sun.clone(), ring.clone()], circ([20.0, 0.0, 0.0], Z, 8.0), "must be 10 mm");
    // No planet drawn: one is made between sun and ring; the carrier is generated.
    let mut it = item("P", ItemKind::Planetary, vec![sun, ring]);
    it.planets = Some(4);
    let g = gear_geoms(&it);
    assert_eq!(g.len(), 3);
    assert_eq!(g[2].radius, Some(10.0));
    assert!((g[2].centre[0].hypot(g[2].centre[1]) - 20.0).abs() < 1e-9);
    let st = stages(std::slice::from_ref(&it), &[]);
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].kind, StageKind::Planetary);
    assert_eq!(st[0].gears.len(), 3);
    assert_eq!((st[0].planets, st[0].carrier), (Some(4), true));
}

#[test]
fn bevel_pair_by_two_circles() {
    // Shafts along z and x meeting at the origin; both cones 36.06 mm long.
    let c1 = circ([0.0, 0.0, 30.0], Z, 20.0);
    let c2 = circ([20.0, 0.0, 0.0], X, 30.0);
    picked(ItemKind::Bevel, &[c1.clone(), c2]);
    refused(ItemKind::Bevel, std::slice::from_ref(&c1), circ([50.0, 0.0, 30.0], Z, 30.0), "parallel");
    refused(ItemKind::Bevel, std::slice::from_ref(&c1), circ([20.0, 5.0, 0.0], X, 30.0), "don't meet");
    refused(ItemKind::Bevel, std::slice::from_ref(&c1), circ([20.0, 0.0, 0.0], X, 25.0), "don't touch");
}

#[test]
fn bevel_pair_by_two_axis_lines_and_one_circle() {
    let (l1, l2) = (line([0.0; 3], [0.0, 0.0, 100.0]), line([0.0; 3], [100.0, 0.0, 0.0]));
    let c = circ([0.0, 0.0, 30.0], Z, 20.0);
    let s = picked(ItemKind::Bevel, &[l1.clone(), l2.clone(), c.clone()]);
    // (Picked one by one the two lines meeting are a side view at first; the circle makes it this layout.)
    assert!(assign(ItemKind::Bevel, &[l1.clone(), l2.clone(), c.clone()]).unwrap().iter().all(|s| s.layout == 1));
    assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["axis", "axis2", "pitch"]);
    // In any order.
    picked(ItemKind::Bevel, &[c.clone(), l2.clone(), l1.clone()]);
    // The mate's pitch circle: through the pitch point (20, 0, 30), round the x axis.
    let it = item("A", ItemKind::Bevel, vec![l1.clone(), l2.clone(), c.clone()]);
    let g = gear_geoms(&it);
    assert_eq!(g.len(), 2);
    assert!(close(g[1].centre, [20.0, 0.0, 0.0]), "{:?}", g[1].centre);
    assert!((g[1].radius.unwrap() - 30.0).abs() < 1e-9);
    refused(ItemKind::Bevel, std::slice::from_ref(&l1), line([10.0, 0.0, 0.0], [10.0, 0.0, 100.0]), "parallel");
    refused(ItemKind::Bevel, &[l1.clone(), l2.clone()], circ([0.0, 30.0, 30.0], Y, 20.0), "not on either axis line");
    refused(ItemKind::Bevel, &[l1.clone(), l2.clone(), c.clone()], line([0.0; 3], [0.0, 100.0, 0.0]), "no room");
    refused(ItemKind::Bevel, &[c.clone(), circ([20.0, 0.0, 0.0], X, 30.0)], l1, "other layout");
    let st = stages(std::slice::from_ref(&it), &[]);
    assert_eq!((st[0].kind, st[0].style), (StageKind::Bevel, Kind::Bevel));
}

#[test]
fn hypoid_pinion_axis_is_offset() {
    let ring = circ([0.0; 3], Z, 50.0);
    picked(ItemKind::Hypoid, &[ring.clone(), line([-80.0, 20.0, 0.0], [-10.0, 20.0, 0.0])]);
    refused(ItemKind::Hypoid, std::slice::from_ref(&ring), line([-80.0, 0.0, 0.0], [-10.0, 0.0, 0.0]), "meets the ring's");
    refused(ItemKind::Hypoid, std::slice::from_ref(&ring), line([-80.0, 60.0, 0.0], [-10.0, 60.0, 0.0]), "nearer");
    refused(ItemKind::Hypoid, std::slice::from_ref(&ring), line([10.0, 0.0, 0.0], [10.0, 0.0, 50.0]), "parallel");
}

#[test]
fn worm_line_tangent_at_the_right_distance() {
    let wheel = circ([0.0; 3], Z, 30.0);
    let worm = line([-40.0, 40.0, 0.0], [40.0, 40.0, 0.0]);
    picked(ItemKind::Worm, &[wheel.clone(), worm.clone()]);
    let it = item("W", ItemKind::Worm, vec![wheel.clone(), worm]);
    let g = gear_geoms(&it);
    assert_eq!(g[1].radius, Some(10.0));
    assert!(close(g[1].centre, [0.0, 40.0, 0.0]));
    refused(ItemKind::Worm, std::slice::from_ref(&wheel), line([-40.0, 25.0, 0.0], [40.0, 25.0, 0.0]), "inside the wheel's pitch radius");
    refused(ItemKind::Worm, std::slice::from_ref(&wheel), line([-40.0, 40.0, 5.0], [40.0, 40.0, 5.0]), "off the wheel's plane");
    refused(ItemKind::Worm, std::slice::from_ref(&wheel), line([-40.0, 40.0, -8.0], [40.0, 40.0, 8.0]), "square (90°)");
    refused(ItemKind::Worm, std::slice::from_ref(&wheel), line([40.0, 0.0, 0.0], [40.0, 0.0, 9.0]), "parallel");
}

#[test]
fn crossed_helical_on_skew_shafts() {
    let c1 = circ([0.0; 3], Z, 20.0);
    picked(ItemKind::CrossedHelical, &[c1.clone(), circ([0.0, 30.0, 0.0], X, 10.0)]);
    refused(ItemKind::CrossedHelical, std::slice::from_ref(&c1), circ([0.0, 30.0, 0.0], X, 15.0), "don't touch");
    refused(ItemKind::CrossedHelical, std::slice::from_ref(&c1), circ([6.0, 30.0, 0.0], X, 10.0), "pass closest");
    refused(ItemKind::CrossedHelical, std::slice::from_ref(&c1), circ([30.0, 0.0, 0.0], Z, 10.0), "parallel");
    refused(ItemKind::CrossedHelical, std::slice::from_ref(&c1), circ([0.0, 0.0, 20.0], X, 10.0), "Bevel Pair");
}

#[test]
fn shaft_with_bearings_on_it() {
    let l = line([0.0; 3], [0.0, 0.0, 100.0]);
    let s = picked(ItemKind::Shaft, &[l.clone(), Thing::point([0.0, 0.0, 10.0]), Thing::point([0.0, 0.0, 90.0])]);
    assert_eq!((s[1].name.as_str(), s[2].name.as_str()), ("bearing", "bearing2"));
    refused(ItemKind::Shaft, std::slice::from_ref(&l), Thing::point([3.0, 0.0, 10.0]), "off the shaft line");
    refused(ItemKind::Shaft, &[], Thing::point([0.0; 3]), "shaft line first");
    let construction = Thing::Line { a: [0.0; 3], b: [0.0, 0.0, 9.0], construction: true, ids: ThingIds::default() };
    refused(ItemKind::Shaft, &[], construction, "construction");
    assert_eq!(GearItem::new("S", ItemKind::Shaft).with(vec![l]).gear_count(), 0);
}

#[test]
fn incomplete_item_is_named() {
    let it = GearItem::new("D", ItemKind::Worm).with(vec![circ([0.0; 3], Z, 30.0)]);
    let e = validate_item(&it).unwrap_err();
    assert!(e.contains("Gear D") && e.contains("worm's axis line"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Module and face
// ---------------------------------------------------------------------------------------------

#[test]
fn module_block_wins_over_own_module() {
    let own = GearSettings { module: Some(1.5), ..GearSettings::default() };
    let blocks = vec![
        ModuleBlock { id: "M1".into(), module: Some(2.0), members: vec![0, 2], ..Default::default() },
        ModuleBlock { id: "M2".into(), module: None, members: vec![3], ..Default::default() },
    ];
    assert_eq!(resolve_module(&blocks, 0, &own), ModuleChoice { module: Some(2.0), from: ModuleFrom::Block(0) });
    assert_eq!(resolve_module(&blocks, 3, &own), ModuleChoice { module: None, from: ModuleFrom::Block(1) });
    assert_eq!(resolve_module(&blocks, 1, &own), ModuleChoice { module: Some(1.5), from: ModuleFrom::Own });
    let teeth = GearSettings { size_by: SizeBy::Teeth, teeth: Some(30), module: Some(1.5), ..GearSettings::default() };
    assert_eq!(resolve_module(&blocks, 1, &teeth).from, ModuleFrom::Teeth);
    assert_eq!(resolve_module(&blocks, 2, &teeth).module, Some(2.0));
    assert_eq!(resolve_module(&[], 0, &GearSettings::default()), ModuleChoice { module: None, from: ModuleFrom::Auto });
}

#[test]
fn face_presets() {
    let f = |preset, a, b| GearSettings { face: FaceSides { preset, a, b }, ..GearSettings::default() };
    assert_eq!(face_extent(&f(FacePreset::Symmetric, None, None), 2.0), (10.0, 10.0));
    assert_eq!(face_extent(&f(FacePreset::Symmetric, Some(4.0), None), 2.0), (4.0, 4.0));
    assert_eq!(face_extent(&f(FacePreset::AOnly, None, None), 2.0), (20.0, 0.0));
    assert_eq!(face_extent(&f(FacePreset::BOnly, None, Some(7.0)), 2.0), (0.0, 7.0));
    assert_eq!(face_extent(&f(FacePreset::Custom, Some(3.0), None), 2.0), (3.0, 0.0));
    assert_eq!(face_extent(&f(FacePreset::Custom, Some(3.0), Some(-1.0)), 2.0), (3.0, 0.0));
    assert_eq!(face_extent(&f(FacePreset::Custom, None, None), 1.0), (5.0, 5.0));
}

// ---------------------------------------------------------------------------------------------
// Suggestions
// ---------------------------------------------------------------------------------------------

#[test]
fn suggestions_from_the_drawing() {
    let items = vec![
        item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 10.0)]),
        // Touching A.
        item("B", ItemKind::Spur, vec![circ([30.0, 0.0, 0.0], Z, 20.0)]),
        // On B's axis, 15 mm up.
        item("C", ItemKind::Helical, vec![circ([30.0, 0.0, 15.0], Z, 12.0)]),
        // Touching nothing.
        item("D", ItemKind::Spur, vec![circ([200.0, 0.0, 0.0], Z, 10.0)]),
        // A ring round D, touching it inside.
        item("E", ItemKind::Internal, vec![circ([200.0, 15.0, 0.0], Z, 25.0)]),
        // A rack under A.
        item("F", ItemKind::Rack, vec![line([-30.0, -10.0, 0.0], [30.0, -10.0, 0.0])]),
    ];
    let s = suggest_links(&items);
    let has = |a: (usize, usize), b: (usize, usize), kind| s.iter().any(|x| x.link().joins(GearRef::new(a.0, a.1), GearRef::new(b.0, b.1), kind));
    assert!(has((0, 0), (1, 0), LinkKind::Mesh), "{s:?}");
    assert!(has((1, 0), (2, 0), LinkKind::Shaft), "{s:?}");
    assert!(has((3, 0), (4, 0), LinkKind::Mesh), "{s:?}");
    assert!(has((0, 0), (5, 0), LinkKind::Mesh), "{s:?}");
    assert_eq!(s.len(), 4, "{s:?}");
    // Accepted ones drop out.
    let left = unlinked(s, &[mesh((1, 0), (0, 0))]);
    assert_eq!(left.len(), 3);
}

#[test]
fn no_suggestions_inside_one_item_or_across_planes() {
    let items = vec![
        item("A", ItemKind::Compound, vec![line([0.0, 0.0, -5.0], [0.0, 0.0, 25.0]), circ([0.0; 3], Z, 20.0), circ([0.0, 0.0, 12.0], Z, 10.0)]),
        // Would touch A's gear 1 but lies 5 mm off its plane.
        item("B", ItemKind::Spur, vec![circ([30.0, 0.0, 5.0], Z, 10.0)]),
    ];
    assert!(suggest_links(&items).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Stages
// ---------------------------------------------------------------------------------------------

/// A (r10) meshes C's gear 1 (r20); C is a compound on x = 30 with gear 2 (r10) 10 mm up,
/// meshing D (r20); E meshes C's gear 1 too.
fn train() -> Vec<GearItem> {
    vec![
        item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 10.0)]),
        item(
            "C",
            ItemKind::Compound,
            vec![line([30.0, 0.0, -5.0], [30.0, 0.0, 15.0]), circ([30.0, 0.0, 0.0], Z, 20.0), circ([30.0, 0.0, 10.0], Z, 10.0)],
        ),
        item("D", ItemKind::Spur, vec![circ([60.0, 0.0, 10.0], Z, 20.0)]),
        item("E", ItemKind::Helical, vec![circ([30.0, 30.0, 0.0], Z, 10.0)]),
    ]
}

#[test]
fn graph_splits_into_stages() {
    let items = train();
    let links = vec![mesh((0, 0), (1, 0)), mesh((1, 1), (2, 0)), mesh((1, 0), (3, 0))];
    let st = stages(&items, &links);
    let meshes: Vec<&Stage> = st.iter().filter(|s| s.kind == StageKind::Parallel).collect();
    assert_eq!(meshes.len(), 3);
    for s in &meshes {
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(s.geom.len(), 2);
    }
    // C gear 1 is in two stages (decision 8).
    assert_eq!(st.iter().filter(|s| s.gears.contains(&GearRef::new(1, 0))).count(), 3);
    assert_eq!(meshes[0].style, Kind::Spur);
    assert_eq!(meshes[2].style, Kind::Helical);
    assert_eq!(meshes[1].from, StageFrom::Link(1));
    let shafts: Vec<&Stage> = st.iter().filter(|s| s.kind == StageKind::Shaft).collect();
    assert_eq!(shafts.len(), 1);
    assert_eq!(shafts[0].gears, vec![GearRef::new(1, 0), GearRef::new(1, 1)]);
}

#[test]
fn explicit_links_decide_and_none_falls_back_to_inference() {
    let items = train();
    // Only one link: only that mesh (plus the compound's cluster).
    let st = stages(&items, &[mesh((0, 0), (1, 0))]);
    assert_eq!(st.iter().filter(|s| s.kind == StageKind::Parallel).count(), 1);
    // No links: the drawing's tangencies are used.
    let st = stages(&items, &[]);
    assert_eq!(st.iter().filter(|s| s.kind == StageKind::Parallel).count(), 3);
    assert!(effective_links(&items, &[]).1);
    assert!(!effective_links(&items, &[mesh((0, 0), (1, 0))]).1);
}

#[test]
fn shaft_links_join_clusters() {
    let items = vec![
        item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 10.0)]),
        item("B", ItemKind::Spur, vec![circ([0.0, 0.0, 20.0], Z, 15.0)]),
        item(
            "C",
            ItemKind::Compound,
            vec![line([0.0, 0.0, 30.0], [0.0, 0.0, 60.0]), circ([0.0, 0.0, 40.0], Z, 10.0), circ([0.0, 0.0, 50.0], Z, 12.0)],
        ),
        item("D", ItemKind::Spur, vec![circ([50.0, 0.0, 0.0], Z, 10.0)]),
    ];
    let st = stages(&items, &[shaft((0, 0), (1, 0)), shaft((1, 0), (2, 1))]);
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].gears.len(), 4);
    assert!(st[0].error.is_none());
    assert_eq!(st[0].from, StageFrom::Cluster { links: vec![0, 1], items: vec![2] });
    // A shaft link between gears on different axes is refused with a hint.
    let st = stages(&items, &[shaft((0, 0), (3, 0))]);
    let bad = st.iter().find(|s| s.gears.contains(&GearRef::new(3, 0))).unwrap();
    assert!(bad.error.as_deref().unwrap().contains("not on one axis"), "{:?}", bad.error);
}

#[test]
fn bad_mesh_links_say_why() {
    let items = vec![
        item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 10.0)]),
        item("B", ItemKind::Spur, vec![circ([40.0, 0.0, 0.0], Z, 20.0)]),
        item("C", ItemKind::Spur, vec![circ([0.0, 0.0, 30.0], Z, 10.0)]),
    ];
    let st = stages(&items, &[mesh((0, 0), (1, 0)), mesh((0, 0), (2, 0)), mesh((0, 0), (5, 0))]);
    assert!(st[0].error.as_deref().unwrap().contains("are 10 mm apart"), "{:?}", st[0].error);
    assert!(st[1].error.as_deref().unwrap().contains("by Shaft"), "{:?}", st[1].error);
    assert!(st[2].error.as_deref().unwrap().contains("doesn't exist"), "{:?}", st[2].error);
}

#[test]
fn every_stage_type_in_one_drivetrain() {
    let mut bevel = item("D", ItemKind::Bevel, vec![circ([0.0, 0.0, 30.0], Z, 20.0), circ([20.0, 0.0, 0.0], X, 30.0)]);
    bevel.gears = vec![GearSettings { bevel_style: BevelStyle::Chevron, ..GearSettings::default() }];
    let items = vec![
        item("A", ItemKind::Spur, vec![circ([200.0, 0.0, 0.0], Z, 10.0)]),
        item("B", ItemKind::Spur, vec![circ([230.0, 0.0, 0.0], Z, 20.0)]),
        item("C", ItemKind::Internal, vec![circ([400.0, 0.0, 0.0], Z, 50.0), circ([430.0, 0.0, 0.0], Z, 20.0)]),
        bevel,
        item("E", ItemKind::Hypoid, vec![circ([0.0, 300.0, 0.0], Z, 50.0), line([-80.0, 320.0, 0.0], [-10.0, 320.0, 0.0])]),
        item("F", ItemKind::Worm, vec![circ([0.0, 600.0, 0.0], Z, 30.0), line([-40.0, 640.0, 0.0], [40.0, 640.0, 0.0])]),
        item("G", ItemKind::CrossedHelical, vec![circ([0.0, 900.0, 0.0], Z, 20.0), circ([0.0, 930.0, 0.0], X, 10.0)]),
        item("H", ItemKind::Planetary, vec![circ([600.0, 0.0, 0.0], Z, 10.0), circ([600.0, 0.0, 0.0], Z, 30.0)]),
        item("I", ItemKind::Rack, vec![line([170.0, -10.0, 0.0], [230.0, -10.0, 0.0])]),
        item("J", ItemKind::Herringbone, vec![circ([230.0, 0.0, 30.0], Z, 20.0)]),
    ];
    let links = vec![mesh((0, 0), (1, 0)), mesh((8, 0), (0, 0)), shaft((1, 0), (9, 0))];
    let st = stages(&items, &links);
    let kinds: Vec<StageKind> = st.iter().map(|s| s.kind).collect();
    for k in [
        StageKind::Parallel,
        StageKind::Internal,
        StageKind::Bevel,
        StageKind::Hypoid,
        StageKind::Worm,
        StageKind::CrossedHelical,
        StageKind::Planetary,
        StageKind::Rack,
        StageKind::Shaft,
    ] {
        assert!(kinds.contains(&k), "{k:?} missing from {kinds:?}");
    }
    for s in &st {
        assert!(s.error.is_none(), "{:?}: {:?}", s.kind, s.error);
    }
    let b = st.iter().find(|s| s.kind == StageKind::Bevel).unwrap();
    assert_eq!((b.style, b.bevel_style), (Kind::SpiralBevel, BevelStyle::Chevron));
    let shaft = st.iter().find(|s| s.kind == StageKind::Shaft).unwrap();
    assert_eq!(shaft.style, Kind::Herringbone);
    // The worm's own pair is its stage once: a duplicate link adds nothing.
    let again = stages(&items, &[mesh((5, 1), (5, 0))]);
    assert_eq!(again.iter().filter(|s| s.kind == StageKind::Worm).count(), 1);
}

#[test]
fn cross_item_worm_and_bevel_links_classify() {
    let items = vec![
        item("A", ItemKind::Worm, vec![circ([0.0; 3], Z, 30.0), line([-40.0, 40.0, 0.0], [40.0, 40.0, 0.0])]),
        // A spur on the worm's own shaft? No: a bevel gear meeting a spur's axis.
        item("B", ItemKind::Spur, vec![circ([0.0, 200.0, 30.0], Z, 20.0)]),
        item("C", ItemKind::Spur, vec![circ([20.0, 200.0, 0.0], X, 30.0)]),
    ];
    let st = stages(&items, &[mesh((1, 0), (2, 0))]);
    let s = st.iter().find(|s| s.from == StageFrom::Link(0)).unwrap();
    assert_eq!(s.kind, StageKind::Bevel);
    assert!(s.error.is_none(), "{:?}", s.error);
}

// ---------------------------------------------------------------------------------------------
// Mapping onto the drivetrain reader
// ---------------------------------------------------------------------------------------------

#[test]
fn mapping_carries_modules_faces_and_meshes() {
    let mut a = item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 10.0), line([0.0, 0.0, -10.0], [0.0, 0.0, 20.0])]);
    a.gears =
        vec![GearSettings { module: Some(1.0), face: FaceSides { preset: FacePreset::AOnly, a: Some(6.0), b: None }, ..GearSettings::default() }];
    let b = item("B", ItemKind::Spur, vec![circ([30.0, 0.0, 0.0], Z, 20.0)]);
    let blocks = vec![ModuleBlock { id: "M1".into(), module: Some(2.0), members: vec![1], ..Default::default() }];
    let m = to_drivetrain(&[a, b], &[mesh((0, 0), (1, 0))], &blocks);
    assert!(!m.inferred);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    // A's gear, A's shaft line, B's gear.
    assert_eq!(m.source, vec![Source::Gear(GearRef::new(0, 0)), Source::Line { item: 0, k: 0 }, Source::Gear(GearRef::new(1, 0))]);
    assert_eq!(m.items[0].ask.module, Some(1.0));
    assert_eq!(m.items[2].ask.module, Some(2.0));
    assert_eq!(m.items[0].ask.face_width, Some(6.0));
    assert_eq!(m.faces[0], Some((6.0, 0.0)));
    assert_eq!(m.faces[2], None);
    assert_eq!(m.meshes, vec![(0, 2)]);
    assert_eq!(m.items[1].ask.role, Some(crate::drivetrain::Role::Shaft));
}

#[test]
fn mapped_train_reads() {
    // The train with no links: inferred, and the reader sizes it.
    let items = train();
    let m = to_drivetrain(&items, &[], &[]);
    assert!(m.inferred);
    assert_eq!(m.meshes.len(), 3);
    assert_eq!(m.shafts.len(), 1);
    // One tooth style per train: E is helical, so drop it for the reader.
    let spur: Vec<GearItem> = items.into_iter().take(3).collect();
    let m = to_drivetrain(&spur, &[], &[]);
    let o = crate::drivetrain::read(&m.items, None, &Simple { face_width: Some(8.0), ..Simple::default() }, &Spec::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    for (k, s) in m.source.iter().enumerate() {
        if let Source::Gear(_) = s {
            assert!(o.members[k].teeth.is_some(), "member {k}: {:?}", o.members[k]);
        }
    }
}

#[test]
fn mapped_worm_and_bevel_read() {
    let worm = item("W", ItemKind::Worm, vec![circ([0.0; 3], Z, 30.0), line([-40.0, 40.0, 0.0], [40.0, 40.0, 0.0])]);
    let m = to_drivetrain(std::slice::from_ref(&worm), &[], &[]);
    assert_eq!(m.items.len(), 2);
    assert_eq!(m.items[1].ask.pitch_diameter, Some(20.0));
    assert!((m.items[1].ask.along.unwrap() - 40.0).abs() < 1e-9);
    assert_eq!(m.meshes, vec![(0, 1)]);
    let o = crate::drivetrain::read(&m.items, None, &Simple::default(), &Spec::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    let bevel =
        item("B", ItemKind::Bevel, vec![line([0.0; 3], [0.0, 0.0, 100.0]), line([0.0; 3], [100.0, 0.0, 0.0]), circ([0.0, 0.0, 30.0], Z, 20.0)]);
    let m = to_drivetrain(std::slice::from_ref(&bevel), &[], &[]);
    assert_eq!(m.items.len(), 2);
    let o = crate::drivetrain::read(&m.items, None, &Simple::default(), &Spec::default());
    assert!(o.error.is_none(), "{:?}", o.error);
}

#[test]
fn mapped_rack_and_planetary_note_their_stage() {
    let rack = item("R", ItemKind::Rack, vec![line([-50.0, -20.0, 0.0], [50.0, -20.0, 0.0]), circ([0.0; 3], Z, 20.0)]);
    let m = to_drivetrain(std::slice::from_ref(&rack), &[], &[]);
    assert_eq!(m.errors.len(), 1);
    assert_eq!(m.items.len(), 1);
    let p = item("P", ItemKind::Planetary, vec![circ([0.0; 3], Z, 10.0), circ([0.0; 3], Z, 30.0)]);
    let m = to_drivetrain(std::slice::from_ref(&p), &[], &[]);
    assert_eq!(m.items.len(), 3);
    assert_eq!(m.items[1].ask.role, Some(crate::drivetrain::Role::Internal));
    assert_eq!(m.meshes.len(), 2);
    assert!(!m.notes.is_empty());
}

/// Three gears on one axis (in three planes) are one shaft: two shaft links are suggested (a
/// chain), not one between every pair; a fourth gear elsewhere on the axis joins with one more.
#[test]
fn gears_on_one_axis_suggest_a_chain_of_shaft_links() {
    let items = vec![
        item("A", ItemKind::Spur, vec![circ([0.0; 3], Z, 10.0)]),
        item("B", ItemKind::Spur, vec![circ([0.0, 0.0, 15.0], Z, 20.0)]),
        item("C", ItemKind::Spur, vec![circ([0.0, 0.0, 30.0], Z, 12.0)]),
    ];
    let s = suggest_links(&items);
    let shafts: Vec<&Suggestion> = s.iter().filter(|x| x.kind == LinkKind::Shaft).collect();
    assert_eq!(shafts.len(), 2, "{s:?}");
    // The two links join all three.
    let joined = |i: usize| shafts.iter().any(|x| x.a.item == i || x.b.item == i);
    assert!(joined(0) && joined(1) && joined(2), "{s:?}");
    let mut four = items.clone();
    four.push(item("D", ItemKind::Spur, vec![circ([0.0, 0.0, 45.0], Z, 8.0)]));
    assert_eq!(suggest_links(&four).iter().filter(|x| x.kind == LinkKind::Shaft).count(), 3);
}

/// A bevel pair drawn in a side view takes its picks in any order: the apex first, a shaft line
/// first, or the contact line between them; a line off the apex is still refused.
#[test]
fn a_bevel_side_view_in_any_order() {
    let apex = Thing::point([0.0; 3]);
    let (s1, s2) = (line([0.0; 3], [60.0, 0.0, 0.0]), line([0.0; 3], [0.0, 0.0, 40.0]));
    let contact = line([0.0; 3], [30.0, 0.0, 20.0]);
    for order in [
        vec![apex.clone(), s1.clone(), s2.clone()],
        vec![s1.clone(), apex.clone(), s2.clone()],
        vec![s1.clone(), s2.clone(), apex.clone()],
        vec![apex.clone(), s1.clone(), s2.clone(), contact.clone()],
    ] {
        let slots = picked(ItemKind::Bevel, &order);
        let names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"apex") && names.contains(&"axis") && names.contains(&"axis2"), "{names:?}");
        assert_eq!(next_slot(ItemKind::Bevel, &order), None, "{names:?}");
    }
    // A shaft line that misses the apex, picked right after it.
    refused(ItemKind::Bevel, &[apex], line([5.0, 5.0, 0.0], [60.0, 5.0, 0.0]), "apex");
}

/// A bore or hub circle is one drawn concentric with a gear's pitch circle and inside it: which
/// gear of a compound it belongs to; one off-centre, too big or on a ring is no detail.
#[test]
fn bore_and_hub_circles_belong_to_the_gear_they_sit_in() {
    let spur = GearItem::new("A", ItemKind::Spur).with(vec![circ([0.0; 3], Z, 20.0)]);
    assert_eq!(detail_gear(&spur, &circ([0.0; 3], Z, 4.0)), Some(0));
    assert_eq!(detail_gear(&spur, &circ([1.0, 0.0, 0.0], Z, 4.0)), None, "off centre");
    assert_eq!(detail_gear(&spur, &circ([0.0; 3], Z, 25.0)), None, "outside the pitch circle");
    assert_eq!(detail_gear(&spur, &circ([0.0, 0.0, 3.0], Z, 4.0)), None, "another plane");
    let compound = GearItem::new("B", ItemKind::Compound).with(vec![
        line([0.0, 0.0, -5.0], [0.0, 0.0, 25.0]),
        circ([0.0; 3], Z, 20.0),
        circ([0.0, 0.0, 12.0], Z, 10.0),
    ]);
    assert_eq!(detail_gear(&compound, &circ([0.0, 0.0, 12.0], Z, 3.0)), Some(1));
    let ring = GearItem::new("C", ItemKind::Internal).with(vec![circ([0.0; 3], Z, 40.0)]);
    assert_eq!(detail_gear(&ring, &circ([0.0; 3], Z, 4.0)), None);
    assert!(takes_details(ItemKind::Herringbone) && !takes_details(ItemKind::Worm));
}

/// A bevel side view clicked as its two shaft lines alone (the corner where they meet picks a
/// line, not the apex point): lines meeting end to end with no circle are the side view, their
/// meeting point the apex; with no contact line its sizes come from the gears' teeth and module.
#[test]
fn two_shaft_lines_meeting_are_a_bevel_side_view() {
    let (s1, s2) = (line([0.0; 3], [0.0, 0.0, 60.0]), line([0.0; 3], [60.0, 0.0, 0.0]));
    let slots = picked(ItemKind::Bevel, &[s1.clone(), s2.clone()]);
    assert_eq!(slots.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["axis", "axis2"]);
    assert_eq!(next_slot(ItemKind::Bevel, &[s1.clone(), s2.clone()]), None, "complete: the apex is where they meet");
    // Sizes from the settings: 20 teeth each at module 2 until they are set (it builds at once),
    // then the ones given.
    let mut it = GearItem::new("A", ItemKind::Bevel).with(vec![s1.clone(), s2.clone()]);
    assert!(validate_item(&it).is_ok());
    // No circle or contact line sizes it: its gears take a module and teeth both.
    assert!(sized_by_settings(ItemKind::Bevel, &it.things));
    let g = gear_geoms(&it);
    assert_eq!(g.len(), 2);
    assert!(g.iter().all(|g| (g.radius.unwrap() - 20.0).abs() < 1e-9), "{g:?}");
    it.gears = vec![
        GearSettings { teeth: Some(20), module: Some(2.0), ..GearSettings::default() },
        GearSettings { teeth: Some(30), ..GearSettings::default() },
    ];
    let g = gear_geoms(&it);
    assert_eq!(g.len(), 2);
    assert!((g[0].radius.unwrap() - 20.0).abs() < 1e-9 && (g[1].radius.unwrap() - 30.0).abs() < 1e-9, "{g:?}");
    assert!(validate_item(&it).is_ok());
    // With a contact line: the ratio from its angle.
    let with_contact = [s1.clone(), s2.clone(), line([0.0; 3], [30.0, 0.0, 20.0])];
    assert_eq!(picked(ItemKind::Bevel, &with_contact)[2].name, "contact");
    assert!(!sized_by_settings(ItemKind::Bevel, &with_contact), "the contact line sizes it");
    assert_eq!(gear_geoms(&GearItem::new("B", ItemKind::Bevel).with(with_contact.to_vec())).len(), 2);
    // Lines that don't meet stay the two-axis-lines-and-a-circle layout.
    let apart = [line([0.0; 3], [0.0, 0.0, 60.0]), line([5.0, 0.0, 0.0], [60.0, 0.0, 0.0])];
    assert_eq!(next_slot(ItemKind::Bevel, &apart), Some("pitch circle"));
}
