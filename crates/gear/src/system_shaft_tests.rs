//! Shafts in a drivetrain of items (plan::system): a Shaft item is a bar along its drawn line on
//! any gear's axis (a spur's, a bevel's), joined to the gears it carries; links through a Shaft
//! item join one rigid cluster; gears of different items on one axis line are suggested a Shaft
//! link; a shaft stops short of a gear it doesn't carry. Drawn as the owner's 4×4 prop shafts:
//! a main gear on the x axis, a prop line each way from it bending 20° into a bevel pair.

use crate::items::{GearItem, GearRef, GearSettings, ItemKind, ItemLink, LinkKind, SizeBy, Thing, suggest_links};
use crate::plan::Spec;
use crate::plan::system::{SystemPlan, plan_items};
use crate::test_util::{bend, g, mesh, planned_system as planned, shaft};

const X: [f64; 3] = [1.0, 0.0, 0.0];

fn spur(id: &str, things: Vec<Thing>) -> GearItem {
    let mut it = GearItem::new(id, ItemKind::Spur).with(things);
    it.gears = vec![GearSettings { module: Some(1.0), ..GearSettings::default() }];
    it
}
fn shaft_item(id: &str, a: [f64; 3], b: [f64; 3]) -> GearItem {
    let mut it = GearItem::new(id, ItemKind::Shaft).with(vec![Thing::line(a, b)]);
    it.shaft_diameter = Some(6.0);
    it
}

/// A side-view bevel pair (two shaft lines from one end), 30 : 30 teeth at module 0.6.
fn bevel(id: &str, x: f64) -> GearItem {
    let mut it = GearItem::new(id, ItemKind::Bevel).with(vec![Thing::line([0.0; 3], [x, 0.0, 0.0]), Thing::line([x, 0.0, 0.0], bend(x))]);
    let s = GearSettings { size_by: SizeBy::Teeth, teeth: Some(30), module: Some(0.6), ..GearSettings::default() };
    it.gears = vec![s, s];
    it
}

/// Items 0 M (z30, on the x axis at the origin), 1 N (meshes M), 2 KF (bevel on +x), 3 KB
/// (bevel on −x); `extra` after them.
fn drawing(m_axis: Option<Thing>, extra: Vec<GearItem>) -> Vec<GearItem> {
    let mut m = vec![Thing::circle([0.0; 3], X, 15.0)];
    m.extend(m_axis);
    let mut items = vec![spur("M", m), spur("N", vec![Thing::circle([0.0, 25.0, 0.0], X, 10.0)]), bevel("KF", 80.0), bevel("KB", -80.0)];
    items.extend(extra);
    items
}

/// The rigid member (`solid::members`) gear `r`'s body is in: its bodies.
fn rigid_of(sp: &SystemPlan, r: GearRef) -> Vec<usize> {
    let b = sp.body(r).unwrap_or_else(|| panic!("{r:?} has no body"));
    crate::plan::solid::members(&sp.plan).into_iter().find(|m| m.bodies.contains(&b)).map(|m| m.bodies).unwrap_or_default()
}

/// The shaft built for Shaft item `item`: its length.
fn shaft_length(sp: &SystemPlan, item: usize) -> f64 {
    let b = sp.body(g(item, 0)).unwrap_or_else(|| panic!("Shaft item {item} has no body"));
    sp.plan.shafts.iter().find(|s| s.body == b).map(|s| s.length).unwrap_or_else(|| panic!("no shaft on body {b}: {:?}", sp.plan.shafts))
}

#[test]
fn every_shaft_item_is_a_bar_along_its_line_joined_to_the_gears_it_carries() {
    // SF1 along the +x prop line, SB1 along the −x one (both through M, opposite ways), SF2 on
    // the front bevel's second shaft (a bevel gear's axis), SB2 on the rear one.
    let items = drawing(
        None,
        vec![
            shaft_item("SF1", [0.0; 3], [80.0, 0.0, 0.0]),
            shaft_item("SB1", [0.0; 3], [-80.0, 0.0, 0.0]),
            shaft_item("SF2", [80.0, 0.0, 0.0], bend(80.0)),
            shaft_item("SB2", [-80.0, 0.0, 0.0], bend(-80.0)),
        ],
    );
    let links = vec![
        mesh(g(0, 0), g(1, 0)),
        shaft(g(0, 0), g(2, 0)),
        shaft(g(0, 0), g(3, 0)),
        shaft(g(0, 0), g(4, 0)),
        shaft(g(0, 0), g(5, 0)),
        shaft(g(2, 1), g(6, 0)),
        shaft(g(3, 1), g(7, 0)),
    ];
    let sp = planned(&items, &links);
    // Four bars, each its own body, named for its item.
    assert_eq!(sp.plan.shafts.len(), 4, "{:?}", sp.plan.shafts);
    for (k, id) in [(4, "SF1"), (5, "SB1"), (6, "SF2"), (7, "SB2")] {
        let b = sp.body(g(k, 0)).unwrap();
        assert!(sp.plan.bodies[b].name.starts_with(&format!("Shaft {id}")), "{}", sp.plan.bodies[b].name);
        let len = shaft_length(&sp, k);
        assert!(len > 10.0 && len <= 80.0 + 1e-9, "{id}: {len} mm");
    }
    // SF1 and SB1 run each way from M along the x axis: their bars along +x and −x.
    let dir = |k: usize| sp.plan.bodies[sp.body(g(k, 0)).unwrap()].frame.z;
    assert!(dir(4)[0] > 1.0 - 1e-9 && dir(5)[0] < -1.0 + 1e-9, "{:?} {:?}", dir(4), dir(5));
    // M, both prop bars and both bevels' first gears are one rigid member; each bevel's second
    // gear turns with its own bar.
    let main = rigid_of(&sp, g(0, 0));
    for r in [g(2, 0), g(3, 0), g(4, 0), g(5, 0)] {
        assert!(main.contains(&sp.body(r).unwrap()), "{r:?} not with M: {main:?}");
    }
    assert!(rigid_of(&sp, g(2, 1)).contains(&sp.body(g(6, 0)).unwrap()));
    assert!(rigid_of(&sp, g(3, 1)).contains(&sp.body(g(7, 0)).unwrap()));
    assert!(!main.contains(&sp.body(g(2, 1)).unwrap()), "the bevel's mate turns apart");
    // Where the prop bar reaches the bend it stops short of the mating bevel (said in the notes).
    assert!(shaft_length(&sp, 4) < 80.0, "SF1 runs into the mate at the bend");
    assert!(sp.notes.iter().any(|n| n.contains("Shaft SF1 is cut back")), "{:?}", sp.notes);
    // The drive turns everything: the bevels' second gears 1 : 1 with M (30 : 30).
    let ratio = |r: GearRef| sp.gears.iter().find(|x| x.item == r.item && x.gear == r.gear).and_then(|x| x.ratio).unwrap_or_else(|| panic!("{r:?}"));
    let (f, b) = (ratio(g(2, 1)), ratio(g(3, 1)));
    assert!((f.abs() - 1.0).abs() < 1e-9 && (b.abs() - 1.0).abs() < 1e-9, "{f} {b}");
}

#[test]
fn coaxial_gears_of_different_items_are_suggested_a_shaft_link_and_used_with_no_links() {
    // M on the x axis at the origin, the bevels' first gears on the prop lines through it: one
    // axis line, drawn in different sketches (a circle and side views).
    let items = drawing(None, Vec::new());
    let s = suggest_links(&items);
    let has = |a: GearRef, b: GearRef| s.iter().any(|x| x.kind == LinkKind::Shaft && ((x.a == a && x.b == b) || (x.a == b && x.b == a)));
    assert!(has(g(0, 0), g(2, 0)) && has(g(0, 0), g(3, 0)), "{s:?}");
    // Three gears on one axis get two links, not three.
    assert!(!has(g(2, 0), g(3, 0)), "{s:?}");
    // With no links, they are used: no "collide ... different stages".
    let sp = planned(&items, &[]);
    assert!(rigid_of(&sp, g(0, 0)).contains(&sp.body(g(2, 0)).unwrap()));
    // A worm's or hypoid pinion's axis line counts too: a spur on the worm's axis.
    let mut w = GearItem::new("W", ItemKind::Worm)
        .with(vec![Thing::circle([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 20.0), Thing::line([-30.0, 25.0, 0.0], [30.0, 25.0, 0.0])]);
    w.gears = vec![GearSettings::default()];
    let p = spur("P", vec![Thing::circle([45.0, 25.0, 0.0], X, 8.0)]);
    let s = suggest_links(&[w, p]);
    assert!(s.iter().any(|x| x.kind == LinkKind::Shaft && x.a == g(0, 1) && x.b == g(1, 0)), "{s:?}");
    // A Shaft item suggests a link to the gear on its line (not to one off its ends).
    let items = drawing(None, vec![shaft_item("S", [-10.0, 0.0, 0.0], [10.0, 0.0, 0.0])]);
    let s = suggest_links(&items);
    let to_s: Vec<GearRef> = s.iter().filter(|x| x.a == g(4, 0)).map(|x| x.b).collect();
    assert_eq!(to_s, vec![g(0, 0)], "{s:?}");
}

#[test]
fn shaft_links_through_a_shaft_item_join_one_rigid_cluster() {
    // M – SF1 and SF1 – KF0, no M – KF0: still one rigid member, no collision refused.
    let items = drawing(None, vec![shaft_item("SF1", [0.0; 3], [80.0, 0.0, 0.0])]);
    let links = vec![mesh(g(0, 0), g(1, 0)), shaft(g(0, 0), g(4, 0)), shaft(g(4, 0), g(2, 0)), shaft(g(0, 0), g(3, 0))];
    let sp = planned(&items, &links);
    let main = rigid_of(&sp, g(0, 0));
    assert!(main.contains(&sp.body(g(2, 0)).unwrap()) && main.contains(&sp.body(g(4, 0)).unwrap()), "{main:?}");
    let f = sp.gears.iter().find(|x| x.item == 2 && x.gear == 1).and_then(|x| x.ratio).unwrap();
    assert!((f.abs() - 1.0).abs() < 1e-9, "{f}");
    // The link replies: through the shaft item each is a 1 : 1 shaft.
    assert!(sp.links[1..3].iter().all(|l| l.error.is_none() && l.ratio.is_some_and(|r| (r - 1.0).abs() < 1e-9)), "{:?}", sp.links);
}

#[test]
fn a_gears_own_shaft_stops_short_of_a_gear_on_its_line_or_carries_it_when_linked() {
    // M takes the +x prop line as its own axis: its shaft runs along it to the bend.
    let axis = Some(Thing::line([0.0; 3], [80.0, 0.0, 0.0]));
    let items = drawing(axis.clone(), Vec::new());
    // Not linked to the bevel on its line: the shaft stops short of it (and of its mate).
    let links = vec![mesh(g(0, 0), g(1, 0)), shaft(g(0, 0), g(3, 0))];
    let sp = planned(&items, &links);
    let m = sp.body(g(0, 0)).unwrap();
    let len = sp.plan.shafts.iter().find(|s| s.body == m).map(|s| s.length).unwrap();
    assert!(len < 78.0, "M's shaft runs into KF1: {len}");
    assert!(sp.notes.iter().any(|n| n.contains("the shaft of Gear M is cut back") && n.contains("Gear KF1")), "{:?}", sp.notes);
    // Linked: the bevel is carried; the shaft runs on through it, short of the mate at the bend.
    let links = vec![mesh(g(0, 0), g(1, 0)), shaft(g(0, 0), g(2, 0)), shaft(g(0, 0), g(3, 0))];
    let sp = planned(&items, &links);
    let m = sp.body(g(0, 0)).unwrap();
    let carried = sp.plan.shafts.iter().find(|s| s.body == m).map(|s| s.length).unwrap();
    assert!(carried > len && carried < 80.0, "{carried} vs {len}");
    assert!(sp.notes.iter().any(|n| n.contains("the shaft of Gear M is cut back") && n.contains("Gear KF2")), "{:?}", sp.notes);
}

#[test]
fn a_shaft_item_that_cant_be_used_is_said_never_dropped() {
    let base = drawing(None, Vec::new());
    let with = |s: GearItem| {
        let mut v = base.clone();
        v.push(s);
        v
    };
    let refused = |items: &[GearItem], links: &[ItemLink], want: &str| {
        let sp = plan_items(items, links, &[], None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
        let e = sp.error.unwrap_or_else(|| panic!("not refused: {want}"));
        assert!(e.contains(want), "{e}");
    };
    let on_m = shaft_item("S", [0.0; 3], [40.0, 0.0, 0.0]);
    let off = shaft_item("S", [0.0, 50.0, 0.0], [40.0, 50.0, 0.0]);
    let core = vec![mesh(g(0, 0), g(1, 0)), shaft(g(0, 0), g(2, 0)), shaft(g(0, 0), g(3, 0))];
    let plus = |l: ItemLink| {
        let mut v = core.clone();
        v.push(l);
        v
    };
    // Linked to a gear off its line; meshed; linked by a gear it hasn't.
    refused(&with(off.clone()), &plus(shaft(g(4, 0), g(1, 0))), "Gear N is not on the line of Shaft S");
    refused(&with(on_m.clone()), &plus(mesh(g(4, 0), g(1, 0))), "Shaft S has no teeth");
    refused(&with(on_m.clone()), &plus(shaft(g(4, 1), g(0, 0))), "Shaft S is linked as gear 1");
    // Linked to nothing and on no gear's axis: made, with a warning.
    let sp = planned(&with(off), &core);
    assert!(sp.plan.warnings.iter().any(|w| w.contains("Shaft S") && w.contains("carries no gear")), "{:?}", sp.plan.warnings);
    // In no link but on M's axis: it carries M, as the notes say.
    let sp = planned(&with(on_m), &core);
    assert!(rigid_of(&sp, g(0, 0)).contains(&sp.body(g(4, 0)).unwrap()));
    assert!(sp.notes.iter().any(|n| n.contains("Shaft S is in no link") && n.contains("Gear M")), "{:?}", sp.notes);
    // Linked to gears past its line's ends (on its axis): it runs on to reach them, as said.
    let short = shaft_item("S", [-40.0, 0.0, 0.0], [-20.0, 0.0, 0.0]);
    let sp = planned(&with(short), &plus(shaft(g(4, 0), g(2, 0))));
    assert!(sp.notes.iter().any(|n| n.contains("Shaft S runs on past its line to") && n.contains("Gear M")), "{:?}", sp.notes);
    assert!(shaft_length(&sp, 4) > 100.0, "{}", shaft_length(&sp, 4));
}
