//! Blocks (crate::blocks): a block's settings reach every member gear that gives none of its own,
//! and a block's shaft Ø makes one shaft per axis, one body with the gears on it.

use crate::blocks::{block_settings, block_shafts, block_spec, chamfer_groups, expand_blocks, only_items, with_blocks};
use crate::items::{
    BlockSettings, Bore, FacePreset, FaceSides, GearItem, GearRef, GearSettings, ItemKind, ItemLink, ModuleBlock, ShaftMode, Thing, gear_geoms,
};
use crate::plan::Spec;
use crate::plan::system::{SystemPlan, plan_items};
use crate::test_util::{close, g, mesh};

const Z: [f64; 3] = [0.0, 0.0, 1.0];

fn spur(id: &str, centre: [f64; 3], r: f64) -> GearItem {
    let mut it = GearItem::new(id, ItemKind::Spur).with(vec![Thing::circle(centre, Z, r)]);
    it.gears = vec![GearSettings::default()];
    it
}

/// A (Ø20 at the origin) meshes B (Ø30 at x 25); C (Ø16) sits on A's axis 12 mm up.
fn drawing() -> Vec<GearItem> {
    vec![spur("A", [0.0; 3], 10.0), spur("B", [25.0, 0.0, 0.0], 15.0), spur("C", [0.0, 0.0, 12.0], 8.0)]
}

fn block(members: Vec<usize>, settings: BlockSettings) -> ModuleBlock {
    ModuleBlock { id: "M1".into(), module: Some(1.0), members, settings }
}

fn planned(items: &[GearItem], links: &[ItemLink], blocks: &[ModuleBlock]) -> (SystemPlan, Vec<usize>) {
    let e = expand_blocks(items, links, blocks);
    let sp = plan_items(&e.items, &e.links, blocks, None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(sp.error.is_none(), "{:?}\nnotes {:?}", sp.error, sp.notes);
    (sp, e.added)
}

/// The bodies of the rigid member gear `r`'s body is in.
fn rigid_of(sp: &SystemPlan, r: GearRef) -> Vec<usize> {
    let b = sp.body(r).unwrap_or_else(|| panic!("{r:?} has no body"));
    crate::plan::solid::members(&sp.plan).into_iter().find(|m| m.bodies.contains(&b)).map(|m| m.bodies).unwrap_or_default()
}

#[test]
fn a_blocks_settings_fill_what_a_member_leaves_on_auto() {
    let b = BlockSettings { helix: Some(15.0), face_width: Some(12.0), shaft_diameter: Some(6.0), ..BlockSettings::default() };
    // All Auto: the block's helix, its face half each side; on a made shaft no bore (the gear
    // and its shaft are one body), Bore only its Ø as the bore.
    let s = block_settings(&b, ItemKind::Spur, &GearSettings::default());
    assert_eq!(s.helix, Some(15.0));
    assert_eq!((s.face.preset, s.face.a, s.face.b), (FacePreset::Symmetric, Some(6.0), None));
    assert_eq!(s.bore, Bore::Auto, "fused to the shaft: no bore");
    let bore_only = BlockSettings { shaft: ShaftMode::BoreOnly, ..b };
    assert_eq!(block_settings(&bore_only, ItemKind::Spur, &GearSettings::default()).bore, Bore::Diameter(6.0));
    // A member's own values stay: its helix, its face, its bore.
    let own = GearSettings {
        helix: Some(0.0),
        face: FaceSides { preset: FacePreset::AOnly, a: Some(4.0), b: None },
        bore: Bore::Solid,
        ..GearSettings::default()
    };
    let s = block_settings(&b, ItemKind::Spur, &own);
    assert_eq!((s.helix, s.face, s.bore), (Some(0.0), own.face, Bore::Solid));
    // An A only face on Auto takes the whole width on side A.
    let a_only = GearSettings { face: FaceSides { preset: FacePreset::AOnly, a: None, b: None }, ..GearSettings::default() };
    assert_eq!(block_settings(&b, ItemKind::Spur, &a_only).face.a, Some(12.0));
    // A bevel takes no helix angle.
    assert_eq!(block_settings(&b, ItemKind::Bevel, &GearSettings::default()).helix, None);
    // Only the members get them, every gear of each; a member Shaft item its Ø.
    let mut items = drawing();
    items.push(GearItem::new("S", ItemKind::Shaft).with(vec![Thing::line([60.0, 0.0, 0.0], [60.0, 0.0, 30.0])]));
    let w = with_blocks(&items, &[block(vec![0, 3], b)]);
    assert_eq!(w[0].settings(0).helix, Some(15.0));
    assert_eq!(w[1], items[1], "not a member: as it was");
    assert_eq!(w[3].shaft_diameter, Some(6.0));
}

#[test]
fn a_module_only_block_changes_nothing_but_the_module() {
    let items = drawing();
    let blocks = [ModuleBlock { id: "M1".into(), module: Some(2.0), members: vec![0, 1], ..ModuleBlock::default() }];
    let e = expand_blocks(&items, &[mesh(g(0, 0), g(1, 0))], &blocks);
    assert_eq!(e.items, items);
    assert_eq!(e.links, vec![mesh(g(0, 0), g(1, 0))]);
    assert!(e.shafts.is_empty() && e.added.is_empty());
    assert!(chamfer_groups(3, &blocks, None).is_empty());
}

#[test]
fn a_blocks_pressure_angle_and_backlash_are_its_gears_spec() {
    let blocks = [block(vec![1], BlockSettings { pressure_angle: Some(25.0), backlash: Some(0.1), ..BlockSettings::default() })];
    let base = Spec::default();
    let s = block_spec(&blocks, &[g(0, 0), g(1, 0)], &base);
    assert!(close(s.pressure_angle, 25.0, 1e-12) && close(s.backlash, 0.1, 1e-12));
    let s = block_spec(&blocks, &[g(0, 0), g(2, 0)], &base);
    assert_eq!((s.pressure_angle, s.backlash), (base.pressure_angle, base.backlash), "no gear in the block: the set's");
    // Planned: the train of A and B takes the block's pressure angle.
    let (sp, _) = planned(&drawing()[..2], &[mesh(g(0, 0), g(1, 0))], &blocks);
    assert!(sp.error.is_none());
}

#[test]
fn a_blocks_tooth_chamfer_goes_on_its_members_and_the_sets_on_the_rest() {
    let blocks = [block(vec![0, 2], BlockSettings { tooth_chamfer: Some(0.3), ..BlockSettings::default() })];
    assert_eq!(chamfer_groups(4, &blocks, Some(0.5)), vec![(0.5, vec![1, 3]), (0.3, vec![0, 2])]);
    assert_eq!(chamfer_groups(4, &blocks, None), vec![(0.3, vec![0, 2])]);
    // The others are emptied (no gears) in their places.
    let only = only_items(&drawing(), &[0, 2]);
    assert_eq!(only.len(), 3);
    assert!(gear_geoms(&only[1]).is_empty() && gear_geoms(&only[2]).len() == 1);
}

#[test]
fn a_blocks_shaft_runs_along_each_axis_over_its_gears() {
    let blocks = [block(vec![0, 1, 2], BlockSettings { shaft_diameter: Some(6.0), ..BlockSettings::default() })];
    let shafts = block_shafts(&drawing(), &blocks);
    assert_eq!(shafts.len(), 2, "A and C on one axis, B on its own: {shafts:?}");
    let ac = shafts.iter().find(|s| s.gears.contains(&g(0, 0))).unwrap();
    assert_eq!(ac.gears, vec![g(0, 0), g(2, 0)]);
    // Module 1, Auto faces 5 mm each side: A from −5 to 5, C from 7 to 17; 6 mm past each end.
    assert!(close(ac.a[2], -11.0, 1e-9) && close(ac.b[2], 23.0, 1e-9), "{ac:?}");
    assert!(close(ac.length(), 34.0, 1e-9) && ac.diameter == 6.0 && ac.existing.is_none());
    let b = shafts.iter().find(|s| s.gears == vec![g(1, 0)]).unwrap();
    assert!(close(b.length(), 22.0, 1e-9) && close(b.a[0], 25.0, 1e-9), "{b:?}");
    // A bearing point on B's axis: its shaft reaches it.
    let mut items = drawing();
    items[1].things.push(Thing::point([25.0, 0.0, 30.0]));
    let b = block_shafts(&items, &blocks).into_iter().find(|s| s.gears == vec![g(1, 0)]).unwrap();
    assert!(close(b.b[2], 30.0, 1e-9) && close(b.a[2], -5.0, 1e-9), "from the gear's face to the bearing: {b:?}");
    // Named S1, S2 across the set; Bore only makes none (the bores only).
    assert_eq!(crate::blocks::shaft_ids(&shafts), vec![Some("S1".to_string()), Some("S2".to_string())]);
    let bore_only = [block(vec![0, 1, 2], BlockSettings { shaft_diameter: Some(6.0), shaft: ShaftMode::BoreOnly, ..BlockSettings::default() })];
    assert!(block_shafts(&drawing(), &bore_only).is_empty());
    let e = expand_blocks(&drawing(), &[], &bore_only);
    assert!(e.added.is_empty() && e.items.len() == 3);
    assert_eq!(e.items[2].settings(0).bore, Bore::Diameter(6.0));
    // A Shaft item already on A's axis takes the Ø: no second shaft there.
    let mut items = drawing();
    items.push(GearItem::new("S", ItemKind::Shaft).with(vec![Thing::line([0.0, 0.0, -20.0], [0.0, 0.0, 30.0])]));
    let e = expand_blocks(&items, &[], &blocks);
    assert_eq!(e.added.len(), 1, "only B's axis gets a new shaft");
    assert_eq!(e.items[3].shaft_diameter, Some(6.0));
}

#[test]
fn a_blocks_shaft_and_the_gears_on_it_are_one_body_per_axis() {
    let blocks = [block(vec![0, 1, 2], BlockSettings { shaft_diameter: Some(6.0), ..BlockSettings::default() })];
    // With links of its own (A meshes B) and with none (inferred from the drawing).
    for links in [vec![mesh(g(0, 0), g(1, 0))], Vec::new()] {
        let (sp, added) = planned(&drawing(), &links, &blocks);
        assert_eq!(added, vec![3, 4]);
        assert_eq!(sp.plan.shafts.len(), 2, "{:?}", sp.plan.shafts);
        assert!(sp.plan.shafts.iter().all(|s| close(s.diameter, 6.0, 1e-9)), "{:?}", sp.plan.shafts);
        let (a, b) = (rigid_of(&sp, g(0, 0)), rigid_of(&sp, g(1, 0)));
        for r in [g(2, 0), g(3, 0)] {
            assert!(a.contains(&sp.body(r).unwrap()), "{r:?} with A: {a:?}");
        }
        assert!(b.contains(&sp.body(g(4, 0)).unwrap()), "B's shaft with B: {b:?}");
        assert!(!a.contains(&sp.body(g(1, 0)).unwrap()), "B turns apart from A");
        assert!(sp.plan.bodies[sp.body(g(3, 0)).unwrap()].name.starts_with("Shaft S1"), "{}", sp.plan.bodies[sp.body(g(3, 0)).unwrap()].name);
    }
}

/// A (Ø20) meshes B (Ø30, x 25); D sits on B's shaft 12 mm up and meshes C, on A's axis: C is
/// driven back round to A's axis at (10/15)·(rd/rc) of A's speed.
fn back_round(rc: f64) -> (Vec<GearItem>, Vec<ItemLink>) {
    let items =
        vec![spur("A", [0.0; 3], 10.0), spur("B", [25.0, 0.0, 0.0], 15.0), spur("C", [0.0, 0.0, 12.0], rc), spur("D", [25.0, 0.0, 12.0], 25.0 - rc)];
    (items, vec![mesh(g(0, 0), g(1, 0)), crate::test_util::shaft(g(1, 0), g(3, 0)), mesh(g(3, 0), g(2, 0))])
}

#[test]
fn a_gear_the_links_turn_at_another_speed_turns_free_on_its_blocks_shaft() {
    let blocks = [block(vec![0, 2], BlockSettings { shaft_diameter: Some(6.0), ..BlockSettings::default() })];
    // C at 4/9 of A's speed: A is on the shaft, C turns free on it in a clearance bore.
    let (items, links) = back_round(15.0);
    let e = expand_blocks(&items, &links, &blocks);
    assert_eq!(e.shafts.len(), 1, "{:?}", e.shafts);
    let s = &e.shafts[0];
    assert_eq!((s.gears.clone(), s.free.clone()), (vec![g(0, 0)], vec![g(2, 0)]), "{s:?}");
    assert!(close(s.length(), 34.0, 1e-9), "the shaft still runs under C: {s:?}");
    assert_eq!(e.items[2].settings(0).bore, Bore::Diameter(6.0 + crate::blocks::FREE_FIT));
    assert_eq!(e.items[0].settings(0).bore, Bore::Auto);
    assert!(!e.links.iter().any(|l| l.a.item == 4 && l.b == g(2, 0)), "no shaft link to C: {:?}", e.links);
    let (sp, added) = planned(&items, &links, &blocks);
    let shaft = sp.body(g(added[0], 0)).unwrap();
    assert!(rigid_of(&sp, g(0, 0)).contains(&shaft), "A is on the shaft");
    assert!(!rigid_of(&sp, g(2, 0)).contains(&shaft), "C is not locked onto it");
    // C at A's speed (rd/rc = 15/10): one shaft for both, as before.
    let (items, links) = back_round(10.0);
    let e = expand_blocks(&items, &links, &blocks);
    assert_eq!((e.shafts[0].gears.clone(), e.shafts[0].free.clone()), (vec![g(0, 0), g(2, 0)], vec![]));
    assert_eq!(e.items[2].settings(0).bore, Bore::Auto);
    let (sp, _) = planned(&items, &links, &blocks);
    assert!(rigid_of(&sp, g(0, 0)).contains(&sp.body(g(2, 0)).unwrap()), "A and C one body on the shaft");
    // Gears the links don't relate stay on the shaft (A and C with no link between them).
    let e = expand_blocks(&items, &[mesh(g(0, 0), g(1, 0))], &blocks);
    assert_eq!(e.shafts[0].gears, vec![g(0, 0), g(2, 0)]);
}

/// A bevel pair drawn as a side view (two shaft lines meeting, no circle) in a block takes the
/// block's module for its size: 15 and 30 teeth at the block's module 1.25 are Ø18.75 and Ø37.5,
/// not Ø30 and Ø60 at the stand-in module 2 (which then couldn't be met: "15 teeth at module
/// 1.25 make Ø18.75, not the Ø30").
#[test]
fn a_side_view_bevel_pair_in_a_block_is_sized_at_the_blocks_module() {
    let mut e = GearItem::new("E", ItemKind::Bevel).with(vec![Thing::line([0.0; 3], [0.0, 0.0, 60.0]), Thing::line([0.0; 3], [60.0, 0.0, 0.0])]);
    e.gears = vec![
        GearSettings { teeth: Some(15), size_by: crate::items::SizeBy::Teeth, ..GearSettings::default() },
        GearSettings { teeth: Some(30), size_by: crate::items::SizeBy::Teeth, ..GearSettings::default() },
    ];
    let blocks = [ModuleBlock { id: "M1".into(), module: Some(1.25), members: vec![0], ..ModuleBlock::default() }];
    let sized = with_blocks(std::slice::from_ref(&e), &blocks);
    let geoms = gear_geoms(&sized[0]);
    assert!(close(2.0 * geoms[0].radius.unwrap(), 18.75, 1e-9) && close(2.0 * geoms[1].radius.unwrap(), 37.5, 1e-9), "{geoms:?}");
    let (sp, _) = planned(&[e], &[], &blocks);
    let got: Vec<(u32, f64, f64)> = sp.gears.iter().map(|r| (r.teeth, r.module, r.pitch_d)).collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got.iter().zip([(15, 18.75), (30, 37.5)]).all(|(g, (z, d))| g.0 == z && close(g.1, 1.25, 1e-9) && close(g.2, d, 1e-6)), "{got:?}");
}

/// A block's Teeth: Helical (its helix angle) or Herringbone (V teeth) on the members left on
/// Auto; a member's own choice (its own helix, 0 for straight) stays.
#[test]
fn a_blocks_herringbone_teeth_go_on_its_members_on_auto() {
    let b = BlockSettings { helix: Some(20.0), herringbone: true, ..BlockSettings::default() };
    let s = block_settings(&b, ItemKind::Spur, &GearSettings::default());
    assert_eq!((s.helix, s.v), (Some(20.0), true));
    let straight = block_settings(&b, ItemKind::Spur, &GearSettings { helix: Some(0.0), ..GearSettings::default() });
    assert_eq!((straight.helix, straight.v), (Some(0.0), false), "its own straight teeth stay");
    let helical = block_settings(&b, ItemKind::Compound, &GearSettings { helix: Some(15.0), ..GearSettings::default() });
    assert_eq!((helical.helix, helical.v), (Some(15.0), false), "its own helical teeth stay");
    let h = BlockSettings { herringbone: false, ..b };
    assert!(!block_settings(&h, ItemKind::Spur, &GearSettings::default()).v);
    // In a block, every gear of a compound gets them.
    let mut items = drawing();
    items[0].kind = ItemKind::Compound;
    items[0].things.push(Thing::circle([0.0, 0.0, 12.0], Z, 6.0));
    let w = with_blocks(&items, &[block(vec![0], b)]);
    assert!((0..2).all(|g| w[0].settings(g).helix == Some(20.0) && w[0].settings(g).v), "{:?}", w[0].gears);
}

/// A side view's bevel pair in a block with a helix: spiral at the block's helix angle
/// (chevron when the block's teeth are herringbone), unless the pair sets its own spiral angle;
/// a hypoid keeps its own spiral, a worm its lead.
#[test]
fn a_blocks_helix_is_its_bevel_pairs_spiral() {
    use crate::items::{BevelStyle, SizeBy};
    let b = BlockSettings { helix: Some(20.0), ..BlockSettings::default() };
    let auto = GearSettings::default();
    let s = block_settings(&b, ItemKind::Bevel, &auto);
    assert_eq!((s.bevel_style, s.spiral_angle, s.helix), (BevelStyle::Spiral, Some(20.0), None));
    let v = block_settings(&BlockSettings { herringbone: true, ..b }, ItemKind::Bevel, &auto);
    assert_eq!((v.bevel_style, v.spiral_angle), (BevelStyle::Chevron, Some(20.0)));
    // Its own: a spiral angle (0 keeps it straight), or a style with no angle (takes the block's).
    let own = GearSettings { spiral_angle: Some(0.0), ..auto };
    assert_eq!(block_settings(&b, ItemKind::Bevel, &own).bevel_style, BevelStyle::Straight);
    let own = GearSettings { spiral_angle: Some(35.0), bevel_style: BevelStyle::Spiral, ..auto };
    assert_eq!(block_settings(&b, ItemKind::Bevel, &own).spiral_angle, Some(35.0));
    let chevron = GearSettings { bevel_style: BevelStyle::Chevron, ..auto };
    assert_eq!(block_settings(&b, ItemKind::Bevel, &chevron), GearSettings { spiral_angle: Some(20.0), ..chevron });
    assert_eq!(block_settings(&BlockSettings { helix: Some(0.0), ..b }, ItemKind::Bevel, &auto), auto, "a helix of 0: straight");
    for kind in [ItemKind::Hypoid, ItemKind::Worm] {
        assert_eq!(block_settings(&b, kind, &auto), auto, "{kind:?}");
    }
    // Planned: the side view's pair is a spiral bevel pair at 20°.
    let mut e = GearItem::new("E", ItemKind::Bevel).with(vec![Thing::line([0.0; 3], [0.0, 0.0, 60.0]), Thing::line([0.0; 3], [60.0, 0.0, 0.0])]);
    e.gears =
        vec![GearSettings { teeth: Some(15), size_by: SizeBy::Teeth, ..auto }, GearSettings { teeth: Some(30), size_by: SizeBy::Teeth, ..auto }];
    let blocks = [ModuleBlock { id: "M1".into(), module: Some(1.25), members: vec![0], settings: b }];
    let w = with_blocks(std::slice::from_ref(&e), &blocks);
    let st = crate::items::stages(&w, &[]);
    assert!(st.iter().any(|s| s.bevel_style == BevelStyle::Spiral), "{st:?}");
    let (sp, _) = planned(&[e], &[], &blocks);
    let spiral = sp.plan.facts.iter().find(|f| f.label.contains("spiral angle")).map(|f| f.value);
    assert!(spiral.is_some_and(|a| close(a, 20.0, 1e-9)), "{:?}", sp.plan.facts);
}

/// A power split: I drives two compounds L and R whose second gears both drive M. Drawn with
/// R's second circle a little off, the loop's speeds don't quite agree; the compounds' layers
/// still turn as one: on the block's shaft, neither layer free (bored) on it.
#[test]
fn a_compounds_layers_are_never_free_on_their_blocks_shaft() {
    let compound = |id: &str, x: f64, r2: f64| {
        let mut it = GearItem::new(id, ItemKind::Compound).with(vec![
            Thing::line([x, 0.0, -6.0], [x, 0.0, 20.0]),
            Thing::circle([x, 0.0, 0.0], Z, 20.0),
            Thing::circle([x, 0.0, 12.0], Z, r2),
        ]);
        it.gears = vec![GearSettings::default(), GearSettings::default()];
        it
    };
    let items = vec![spur("I", [0.0; 3], 10.0), compound("L", 30.0, 10.0), compound("R", -30.0, 10.02), spur("M", [0.0, 0.0, 12.0], 20.0)];
    let links = vec![mesh(g(0, 0), g(1, 0)), mesh(g(0, 0), g(2, 0)), mesh(g(1, 1), g(3, 0)), mesh(g(2, 1), g(3, 0))];
    let blocks = [block(vec![0, 1, 2, 3], BlockSettings { shaft_diameter: Some(5.0), ..BlockSettings::default() })];
    let e = expand_blocks(&items, &links, &blocks);
    for c in [1, 2] {
        let s = e.shafts.iter().find(|s| s.gears.contains(&g(c, 0)) || s.free.contains(&g(c, 0))).unwrap();
        assert_eq!((s.gears.clone(), s.free.clone()), (vec![g(c, 0), g(c, 1)], vec![]), "{s:?}");
        assert!((0..2).all(|k| e.items[c].settings(k).bore == Bore::Auto), "{c}: {:?}", e.items[c].gears);
    }
    // (M, on I's axis at another speed, does turn free on I's shaft.)
    let s = e.shafts.iter().find(|s| s.gears.contains(&g(0, 0))).unwrap();
    assert_eq!(s.free, vec![g(3, 0)], "{s:?}");
}

/// A side view's bevel pair in a block with Make shaft, with a Shaft item already drawn along
/// the pinion's axis but stopping short of it (from 25 mm out; the pinion's back is at 18.75):
/// that shaft takes the block's Ø, reaches into the pinion and is one body with it, with the
/// request's links and without.
#[test]
fn a_shaft_line_already_on_a_bevels_axis_reaches_it_and_is_one_body_with_it() {
    use crate::items::SizeBy;
    let mut e = GearItem::new("E", ItemKind::Bevel).with(vec![Thing::line([0.0; 3], [0.0, 0.0, 60.0]), Thing::line([0.0; 3], [60.0, 0.0, 0.0])]);
    e.gears = vec![
        GearSettings { teeth: Some(15), size_by: SizeBy::Teeth, ..GearSettings::default() },
        GearSettings { teeth: Some(30), size_by: SizeBy::Teeth, ..GearSettings::default() },
    ];
    let s = GearItem::new("S", ItemKind::Shaft).with(vec![Thing::line([0.0, 0.0, 25.0], [0.0, 0.0, 60.0])]);
    let items = vec![e, s];
    let blocks = [ModuleBlock {
        id: "M1".into(),
        module: Some(1.25),
        members: vec![0, 1],
        settings: BlockSettings { shaft_diameter: Some(5.0), ..BlockSettings::default() },
    }];
    for links in [Vec::new(), vec![mesh(g(0, 0), g(0, 1))]] {
        let (sp, added) = planned(&items, &links, &blocks);
        assert_eq!(added.len(), 1, "only the gear's axis gets a new shaft: {added:?}");
        let shaft = sp.body(g(1, 0)).unwrap();
        assert!(rigid_of(&sp, g(0, 0)).contains(&shaft), "links {links:?}: S with the pinion {:?}", rigid_of(&sp, g(0, 0)));
        let sh = sp.plan.shafts.iter().find(|x| x.body == shaft).unwrap();
        assert!(close(sh.diameter, 5.0, 1e-9), "{sh:?}");
        // Its bar reaches below the pinion's back face (z 18.75) into the gear.
        let (lo, _) = crate::mesh3d::Solid::new(&sp.plan.bodies[shaft], 1.0).bounds();
        assert!(lo[2] < 17.0, "links {links:?}: the shaft starts at z {}", lo[2]);
    }
}

/// The RC 4x4's front and rear bevel pairs as side views: apexes at x ±85 on the prop shaft's
/// line, each wheel shaft 148° from it, 15 : 15 teeth in a block at module 1.25 with Make shaft
/// Ø10 and no shaft drawn. Each block shaft stops in its bevel gear on the apex side (past it the
/// mate's shaft crosses) and runs on out of the gear's back: none is cut back off its gears.
#[test]
fn a_block_shaft_stops_in_a_bevel_gear_at_its_apex_side_and_reaches_it() {
    let pair = |id: &str, apex: [f64; 3], s: f64| {
        let a = 148f64.to_radians();
        let inward = [apex[0] - s * 60.0, apex[1], apex[2]];
        let wheel = [apex[0] + s * 60.0 * -a.cos(), apex[1], apex[2] + 60.0 * a.sin()];
        let mut it = GearItem::new(id, ItemKind::Bevel).with(vec![Thing::line(apex, inward), Thing::line(apex, wheel)]);
        let gs = GearSettings { teeth: Some(15), ..GearSettings::default() };
        it.gears = vec![gs, gs];
        it
    };
    let items = vec![pair("A", [-85.0, 0.0, 0.0], -1.0), pair("B", [85.0, 0.0, 0.0], 1.0)];
    let settings = BlockSettings { shaft_diameter: Some(10.0), ..BlockSettings::default() };
    let blocks = [ModuleBlock { id: "M1".into(), module: Some(1.25), members: vec![0, 1], settings }];
    let shafts = block_shafts(&items, &blocks);
    assert_eq!(shafts.len(), 3, "the prop shaft and two wheel shafts: {shafts:?}");
    // The prop shaft ends inside each pinion, short of the apex (x ±85), and the wheel shafts
    // start inside theirs and run outward.
    let prop = shafts.iter().find(|s| s.a[2].abs() < 1e-6 && s.b[2].abs() < 1e-6).expect("prop shaft");
    let (x0, x1) = (prop.a[0].min(prop.b[0]), prop.a[0].max(prop.b[0]));
    assert!(x0 > -85.0 && x0 < -82.3 && x1 < 85.0 && x1 > 82.3, "prop shaft x {x0} .. {x1}");
    let (sp, _) = planned(&items, &[], &blocks);
    assert!(!sp.notes.iter().any(|n| n.contains("cut back")), "{:?}", sp.notes);
}
