//! Messages name the real gear (its item and number), and a shaft a gear sits on is called a
//! shaft, not a bore.

use crate::blocks::expand_blocks;
use crate::items::{BlockSettings, GearItem, GearSettings, ItemKind, ItemLink, ModuleBlock, SizeBy, Thing};
use crate::plan::Spec;
use crate::plan::system::{SystemPlan, plan_items};
use crate::test_util::{g, mesh};

const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// A bevel pair drawn as a side view: two shaft lines from `apex`, `angle`° apart, `z` teeth each.
fn side_bevel(id: &str, apex: [f64; 3], angle: f64, z: u32) -> GearItem {
    let a = angle.to_radians();
    let tip = |d: [f64; 3]| [apex[0] + 60.0 * d[0], apex[1] + 60.0 * d[1], apex[2] + 60.0 * d[2]];
    let mut e = GearItem::new(id, ItemKind::Bevel).with(vec![Thing::line(apex, tip(Z)), Thing::line(apex, tip([a.sin(), 0.0, a.cos()]))]);
    let s = GearSettings { teeth: Some(z), size_by: SizeBy::Teeth, ..GearSettings::default() };
    e.gears = vec![s, s];
    e
}

fn planned(items: &[GearItem], links: &[ItemLink], blocks: &[ModuleBlock]) -> SystemPlan {
    let e = expand_blocks(items, links, blocks);
    plan_items(&e.items, &e.links, blocks, None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"))
}

fn make_shaft(d: f64) -> BlockSettings {
    BlockSettings { shaft_diameter: Some(d), ..BlockSettings::default() }
}

fn spur(id: &str, centre: [f64; 3], r: f64) -> GearItem {
    let mut it = GearItem::new(id, ItemKind::Spur).with(vec![Thing::circle(centre, Z, r)]);
    it.gears = vec![GearSettings::default()];
    it
}

/// The RC 4x4's bevel pairs E and F drawn as side views at 160°, 15 : 15 teeth at the block's
/// module 1.25 (cones too flat for those teeth): each refusal names its own pinion ("Gear F1"),
/// not "gear 1", and not both gears of the set in front of it.
#[test]
fn a_side_view_bevel_pairs_refusal_names_its_gear() {
    let items = vec![side_bevel("E", [0.0; 3], 160.0, 15), side_bevel("F", [200.0, 0.0, 0.0], 160.0, 15)];
    let blocks = [ModuleBlock { id: "M1".into(), module: Some(1.25), members: vec![0, 1], settings: make_shaft(15.0) }];
    let sp = planned(&items, &[], &blocks);
    let tall = "the teeth are too tall for its cone (pitch cone angle 80°): at this angle it needs at least 23 teeth, it has 15. The module doesn't change this, since the whole gear scales with it: give both gears more teeth at the same ratio, or change the shaft angle";
    assert_eq!(sp.error.as_deref(), Some(format!("Gear E1: {tall}; Gear F1: {tall}").as_str()));
}

/// A block's Make shaft Ø15 through an 11-tooth gear at module 1.25 (roots about Ø10.6 to
/// Ø11.5): the gear is one body with the shaft, so it is the shaft that is too wide, named as
/// the block's (and as its Shaft item where the block made one), with the widest that fits.
#[test]
fn a_block_shaft_wider_than_a_gears_roots_is_said_as_the_shaft() {
    let blocks = [ModuleBlock { id: "M1".into(), module: Some(1.25), members: vec![0, 1], settings: make_shaft(15.0) }];
    // Spur gears: the block makes shafts S1 and S2, B on S2.
    let sp = planned(&[spur("A", [0.0; 3], 12.5), spur("B", [19.375, 0.0, 0.0], 6.875)], &[], &blocks);
    assert_eq!(
        sp.error.as_deref(),
        Some("Gear B sits on Block 1's shaft S2 (Ø15), which is wider than its tooth roots (Ø10.6): use a shaft of at most Ø8 or a bigger gear")
    );
    // A compound (its own shaft line, which takes the block's Ø): its 11-tooth layer, profile
    // shifted to the drawn centre distance.
    let mut b = GearItem::new("B", ItemKind::Compound).with(vec![
        Thing::line([19.375, 0.0, -6.0], [19.375, 0.0, 20.0]),
        Thing::circle([19.375, 0.0, 0.0], Z, 6.875),
        Thing::circle([19.375, 0.0, 12.0], Z, 15.0),
    ]);
    b.gears = vec![GearSettings::default(), GearSettings::default()];
    for links in [Vec::new(), vec![mesh(g(0, 0), g(1, 0))]] {
        let sp = planned(&[spur("A", [0.0; 3], 12.5), b.clone()], &links, &blocks);
        assert_eq!(
            sp.error.as_deref(),
            Some("Gear B1 sits on Block 1's shaft (Ø15), which is wider than its tooth roots (Ø11.5): use a shaft of at most Ø9 or a bigger gear"),
            "links {links:?}"
        );
    }
    // A Shaft item of its own (no block): "shaft S".
    let s = GearItem {
        shaft_diameter: Some(15.0),
        ..GearItem::new("S", ItemKind::Shaft).with(vec![Thing::line([19.375, 0.0, -10.0], [19.375, 0.0, 10.0])])
    };
    let sp = planned(
        &[spur("A", [0.0; 3], 12.5), spur("B", [19.375, 0.0, 0.0], 6.875), s],
        &[],
        &[ModuleBlock { id: "M1".into(), module: Some(1.25), members: vec![0, 1], ..ModuleBlock::default() }],
    );
    assert_eq!(
        sp.error.as_deref(),
        Some("Gear B sits on shaft S (Ø15), which is wider than its tooth roots (Ø10.6): use a shaft of at most Ø8 or a bigger gear")
    );
    assert_eq!(crate::hub::largest_shaft(12.099, 1.25), 9.0);
    assert_eq!(crate::hub::largest_shaft(4.0, 0.5), 2.0);
}

/// A real bore (the gear's own) too big for the roots keeps the bore wording.
#[test]
fn a_bore_wider_than_the_roots_is_said_as_the_bore() {
    let s = Spec { bore: crate::hub::Bore { kind: crate::hub::BoreKind::Round, diameter: 36.0, ..Default::default() }, ..Spec::default() };
    assert_eq!(crate::plan::plan(&s).err().as_deref(), Some("gear: the bore (reaching Ø36) cuts into the tooth roots (Ø35); use a smaller bore"));
}

/// Teeth too tall for a flat cone: the module scales tooth and cone alike, so the refusal names
/// the fewest teeth instead (23 at a 160° pair's 80° cones), at any module, and 23 teeth pass.
#[test]
fn a_flat_cones_refusal_names_the_fewest_teeth_at_any_module() {
    for m in [0.6, 1.25, 3.0] {
        let block = |z: u32| {
            let blocks = [ModuleBlock { id: "M1".into(), module: Some(m), members: vec![0], settings: BlockSettings::default() }];
            planned(&[side_bevel("E", [0.0; 3], 160.0, z)], &[], &blocks).error.unwrap_or_default()
        };
        let few = block(22);
        assert!(few.contains("needs at least 23 teeth, it has 22") && few.contains("module doesn't change this"), "m{m}: {few}");
        assert!(!block(23).contains("too tall"), "m{m}: {}", block(23));
    }
}
