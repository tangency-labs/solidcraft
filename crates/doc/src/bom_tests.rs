use serde_json::json;

use super::*;
use crate::BomType::{Inseparable, Normal, Phantom, Purchased, Reference};
use crate::{IDENTITY, Occurrence};

/// A new component placed once in `parent`, and that placement.
fn add(doc: &mut Document, name: &str, parent: u64) -> (u64, u64) {
    let component = doc.add_component(Some(name), parent).unwrap();
    (component, doc.occurrences.last().unwrap().id)
}

/// Another placement of `component` in `parent`.
fn place(doc: &mut Document, component: u64, parent: u64) -> u64 {
    doc.add_occurrence(component, parent, IDENTITY).unwrap()
}

/// A part property as a file holds it (reading never validates).
fn set(doc: &mut Document, component: u64, key: &str, value: &str) {
    doc.components.iter_mut().find(|c| c.id == component).unwrap().metadata.0.insert(key.into(), json!(value));
}

fn number(doc: &mut Document, component: u64, part_number: &str) {
    set(doc, component, "part_number", part_number);
}

fn typed(doc: &mut Document, component: u64, bom_type: BomType) {
    set(doc, component, "bom_type", bom_type.as_str());
}

/// What `request` lists for `doc` (none of whose components has bodies), with each row's members.
fn listed(doc: &Document, request: Request) -> Result<Bom> {
    let state = ModelState::default();
    list_with(&mut Tree::new(doc, &state), &request, true)
}

fn view(view: View) -> Request {
    Request { view, ..Request::default() }
}

fn folded(view: View) -> Request {
    Request { view, fold: true, ..Request::default() }
}

/// Each row, followed by the rows under it: item, names, type, count and each.
fn rows(bom: &Bom) -> Vec<(String, String, BomType, u64, u64)> {
    bom.rows_deep().into_iter().map(|(item, r)| (item_text(&item), r.names.join("; "), r.bom_type, r.count, r.each)).collect()
}

fn row(item: &str, names: &str, bom_type: BomType, count: u64, each: u64) -> (String, String, BomType, u64, u64) {
    (item.into(), names.into(), bom_type, count, each)
}

#[test]
fn the_view_and_grouping_words_are_the_commands() {
    for v in View::ALL {
        assert_eq!(View::parse(v.as_str()), Some(v));
    }
    for g in Group::ALL {
        assert_eq!(Group::parse(g.as_str()), Some(g));
    }
    assert_eq!((View::default().as_str(), Group::default().as_str()), ("flat", "part_number"), "what assembly.bom lists by default");
    for word in ["", "deep", "Flat", "part number"] {
        assert_eq!((View::parse(word), Group::parse(word)), (None, None), "{word}");
    }
}

#[test]
fn an_empty_assembly_lists_nothing_and_weighs_nothing() {
    let mut doc = Document::default();
    let empty = listed(&doc, Request { mass: true, ..Request::default() }).unwrap();
    assert!(empty.rows.is_empty());
    assert_eq!(empty.total, Total { occurrences: 0, mass: Mass::Unknown(Unknown::NothingToWeigh) });
    assert_eq!(listed(&doc, Request::default()).unwrap().total.mass, Mass::NotAsked, "weighed only when asked");
    // A component that holds nothing, as the assembly listed; one that isn't there is refused.
    let (part, _) = add(&mut doc, "Part", 0);
    for v in View::ALL {
        assert!(listed(&doc, Request { source: part, view: v, ..Request::default() }).unwrap().rows.is_empty());
    }
    assert!(listed(&doc, Request { source: 999, ..Request::default() }).is_err());
}

#[test]
fn one_part_is_one_row_named_by_its_lowest_member() {
    let mut doc = Document::default();
    let (part, occurrence) = add(&mut doc, "Part", 0);
    for (v, stands_for) in [(View::PartsOnly, Holds::Own), (View::FirstLevel, Holds::All)] {
        let bom = listed(&doc, view(v)).unwrap();
        let [r] = bom.rows.as_slice() else { panic!("{bom:?}") };
        assert_eq!(r.key, RowKey { parents: vec![], member: Member { component: part, bom_type: Normal } });
        assert_eq!((r.item, r.count, r.each, r.stands_for, r.mass), (1, 1, 1, stands_for, Mass::NotAsked));
        assert_eq!((&r.components, &r.names, &r.members), (&vec![part], &vec!["Part".to_string()], &vec![vec![occurrence]]));
        assert!(r.children.is_empty() && r.materials.is_empty() && r.part_number.is_empty());
        assert_eq!(bom.total.occurrences, 1);
    }
}

#[test]
fn a_row_is_named_by_its_lowest_member_not_by_its_part_number() {
    let mut doc = Document::default();
    let (a, oa) = add(&mut doc, "A", 0);
    let (b, ob) = add(&mut doc, "B", 0);
    number(&mut doc, a, "P");
    number(&mut doc, b, "P");
    let bom = listed(&doc, Request::default()).unwrap();
    let [r] = bom.rows.as_slice() else { panic!("{bom:?}") };
    assert_eq!((r.key.member.component, &r.components, &r.members, r.part_number.as_str()), (a, &vec![a, b], &vec![vec![oa], vec![ob]], "P"));
    // Renumbering the lower one: its row goes with it, and the other's row is named by the other.
    number(&mut doc, a, "Q");
    let bom = listed(&doc, Request::default()).unwrap();
    let keys: Vec<(&str, u64)> = bom.rows.iter().map(|r| (r.part_number.as_str(), r.key.member.component)).collect();
    assert_eq!(keys, [("P", b), ("Q", a)]);
}

#[test]
fn one_part_number_counted_as_two_types_is_two_rows() {
    let mut doc = Document::default();
    let (a, _) = add(&mut doc, "Made", 0);
    let (b, _) = add(&mut doc, "Bought", 0);
    number(&mut doc, a, "P");
    number(&mut doc, b, "P");
    typed(&mut doc, b, Purchased);
    // A second placement of the made one, counted as bought: it joins the bought row.
    let second = place(&mut doc, a, 0);
    doc.occurrences.iter_mut().find(|o| o.id == second).unwrap().bom_type = Some(Purchased);
    for v in View::ALL {
        let bom = listed(&doc, view(v)).unwrap();
        assert_eq!(rows(&bom), [row("1", "Made", Normal, 1, 1), row("2", "Made; Bought", Purchased, 2, 2)], "{v:?}");
        let keys: Vec<Member> = bom.rows.iter().map(|r| r.key.member).collect();
        assert_eq!(keys, [Member { component: a, bom_type: Normal }, Member { component: a, bom_type: Purchased }]);
    }
}

#[test]
fn a_phantom_in_a_phantom_promotes_what_it_holds_into_the_level_it_sits_in() {
    let mut doc = Document::default();
    let (kit, okit) = add(&mut doc, "Kit", 0);
    let (set_, oset) = add(&mut doc, "Set", kit);
    let oset2 = place(&mut doc, set_, kit);
    let (screw, oscrew) = add(&mut doc, "Screw", set_);
    let oscrew2 = place(&mut doc, screw, set_);
    let alone = place(&mut doc, screw, 0);
    typed(&mut doc, kit, Phantom);
    typed(&mut doc, set_, Phantom);
    let mut paths = vec![vec![okit, oset, oscrew], vec![okit, oset, oscrew2], vec![okit, oset2, oscrew], vec![okit, oset2, oscrew2], vec![alone]];
    paths.sort();
    for v in View::ALL {
        let bom = listed(&doc, view(v)).unwrap();
        assert_eq!(rows(&bom), [row("1", "Screw", Normal, 5, 5)], "{v:?}");
        assert_eq!(bom.rows[0].members, paths);
    }
}

#[test]
fn a_reference_counts_nowhere_and_its_own_list_lists_what_it_holds() {
    let mut doc = Document::default();
    let (fixture, ofixture) = add(&mut doc, "Fixture", 0);
    add(&mut doc, "Pin", fixture);
    typed(&mut doc, fixture, Reference);
    for v in View::ALL {
        let bom = listed(&doc, Request { mass: true, ..view(v) }).unwrap();
        assert_eq!((bom.rows.len(), bom.total.occurrences), (0, 0), "{v:?}");
        assert_eq!(bom.total.mass, Mass::Unknown(Unknown::NothingToWeigh));
        assert_eq!(rows(&listed(&doc, Request { source: fixture, view: v, ..Request::default() }).unwrap()), [row("1", "Pin", Normal, 1, 1)]);
    }
    // An occurrence counted as a reference, its component normal: the same.
    set(&mut doc, fixture, "bom_type", "normal");
    assert_eq!(rows(&listed(&doc, Request::default()).unwrap()), [row("1", "Pin", Normal, 1, 1)]);
    doc.occurrences.iter_mut().find(|o| o.id == ofixture).unwrap().bom_type = Some(Reference);
    assert!(listed(&doc, Request::default()).unwrap().rows.is_empty());
}

#[test]
fn a_list_walks_64_levels_and_refuses_a_65th() {
    let mut doc = Document::default();
    let mut parent = 0;
    for level in 1..=64 {
        parent = add(&mut doc, &format!("L{level}"), parent).0;
    }
    let bom = listed(&doc, Request::default()).unwrap();
    assert_eq!(rows(&bom), [row("1", "L64", Normal, 1, 1)]);
    assert_eq!(bom.rows[0].members[0].len(), 64);
    add(&mut doc, "L65", parent);
    assert!(listed(&doc, Request::default()).is_err(), "refused, never listed in part");
    // The first level only walks what the design places directly.
    assert_eq!(rows(&listed(&doc, view(View::FirstLevel)).unwrap()), [row("1", "L1", Normal, 1, 1)]);
}

#[test]
fn one_placement_budget_covers_the_whole_list_folded_parts_included() {
    let mut doc = Document::default();
    let (a, _) = add(&mut doc, "A", 0);
    for _ in 1..1000 {
        place(&mut doc, a, 0);
    }
    let (b, _) = add(&mut doc, "B", a);
    for _ in 1..99 {
        place(&mut doc, b, a);
    }
    // 1,000 As holding 99 Bs each: 100,000 placements, as many as one list walks.
    let flat = listed(&doc, Request::default()).unwrap();
    assert_eq!((rows(&flat), flat.total.occurrences), (vec![row("1", "B", Normal, 99_000, 99_000)], 99_000));
    number(&mut doc, a, "A-1");
    let kit = listed(&doc, folded(View::PartsOnly)).unwrap();
    assert_eq!((rows(&kit), kit.total.occurrences), (vec![row("1", "A", Normal, 1000, 1000), row("1.1", "B", Normal, 99_000, 99)], 100_000));
    let (a_row, b_row) = (&kit.rows[0], &kit.rows[0].children[0]);
    assert_eq!(b_row.key, RowKey { parents: vec![a_row.key.member], member: Member { component: b, bom_type: Normal } });
    assert_eq!(a_row.stands_for, Holds::All, "a folded part's row is the kit");
    // One placement more, anywhere, is refused, folded or not: a folded part's contents are walked
    // under the list's budget, not one of their own.
    add(&mut doc, "C", 0);
    for request in [Request::default(), folded(View::PartsOnly), folded(View::FirstLevel)] {
        assert!(listed(&doc, request).is_err(), "{request:?}");
    }
    assert_eq!(listed(&doc, view(View::FirstLevel)).unwrap().total.occurrences, 1001, "the first level walks 1,001");
}

/// Kits used to be compared as text (`"#12×1"`): a part without a part number, whose id is 12,
/// read the same as a part numbered "#12", and a part numbered "P×1\nQ" the same as a P and a Q.
#[test]
fn kits_are_told_apart_by_what_they_hold_not_by_how_it_reads() {
    let mut doc = Document::default();
    let bolt = |doc: &mut Document, name: &str| {
        let (id, _) = add(doc, name, 0);
        number(doc, id, "BOLT");
        id
    };
    let (b1, b2, b3, b4) = (bolt(&mut doc, "Bolt 1"), bolt(&mut doc, "Bolt 2"), bolt(&mut doc, "Bolt 3"), bolt(&mut doc, "Bolt 4"));
    let (plain, _) = add(&mut doc, "Plain", b1);
    let (spelled, _) = add(&mut doc, "Spelled", b2);
    number(&mut doc, spelled, &format!("#{plain}"));
    let (pq, _) = add(&mut doc, "PQ", b3);
    number(&mut doc, pq, "P×1\nQ");
    let (p, _) = add(&mut doc, "P", b4);
    let (q, _) = add(&mut doc, "Q", b4);
    number(&mut doc, p, "P");
    number(&mut doc, q, "Q");
    let bolts = |doc: &Document| -> Vec<Vec<u64>> {
        let bom = listed(doc, folded(View::PartsOnly)).unwrap();
        bom.rows.iter().filter(|r| r.part_number == "BOLT").map(|r| r.components.clone()).collect()
    };
    let mut each: Vec<Vec<u64>> = bolts(&doc);
    each.sort();
    assert_eq!(each, [vec![b1], vec![b2], vec![b3], vec![b4]], "four kits, four rows");
    // The same parts by part number are one kit, whichever definitions they are.
    let b5 = bolt(&mut doc, "Bolt 5");
    let (p2, _) = add(&mut doc, "P again", b5);
    let (q2, _) = add(&mut doc, "Q again", b5);
    number(&mut doc, p2, "P");
    number(&mut doc, q2, "Q");
    assert!(bolts(&doc).contains(&vec![b4, b5]));
    let bom = listed(&doc, folded(View::PartsOnly)).unwrap();
    let both = bom.rows.iter().find(|r| r.components == [b4, b5]).unwrap();
    let inside: Vec<(&str, &[u64], u64, u64)> =
        both.children.iter().map(|c| (c.part_number.as_str(), c.components.as_slice(), c.each, c.count)).collect();
    assert_eq!(inside, [("P", &[p, p2][..], 1, 2), ("Q", &[q, q2][..], 1, 2)], "each bolt's, counted for both");
    // The same parts counted as another type are another kit: a bolt whose Q is bought is a row of its own.
    let b6 = bolt(&mut doc, "Bolt 6");
    let (p3, _) = add(&mut doc, "P third", b6);
    let (q3, bought) = add(&mut doc, "Q third", b6);
    number(&mut doc, p3, "P");
    number(&mut doc, q3, "Q");
    doc.occurrences.iter_mut().find(|o| o.id == bought).unwrap().bom_type = Some(Purchased);
    let each = bolts(&doc);
    assert!(each.contains(&vec![b4, b5]) && each.contains(&vec![b6]), "{each:?}");
}

/// All levels walks under the list's one budget: 100,000 placements and 64 levels are listed, and
/// one more of either is refused, never listed in part.
#[test]
fn all_levels_lists_at_each_cap_and_refuses_one_more() {
    let mut doc = Document::default();
    let (part, _) = add(&mut doc, "Part", 0);
    let base = doc.occurrences.iter().map(|o| o.id).max().unwrap_or(0);
    let placement =
        |id: u64| Occurrence { id, component: part, parent: 0, name: format!("Part:{id}"), transform: IDENTITY, grounded: false, bom_type: None };
    doc.occurrences.extend((1..MAX_PLACEMENTS as u64).map(|i| placement(base + i)));
    let all = listed(&doc, view(View::AllLevels)).unwrap();
    assert_eq!((all.rows.len(), all.total.occurrences), (1, MAX_PLACEMENTS as u64));
    doc.occurrences.push(placement(base + MAX_PLACEMENTS as u64));
    assert!(listed(&doc, view(View::AllLevels)).is_err(), "100,001 placements");
    let mut doc = Document::default();
    let mut parent = 0;
    for i in 0..MAX_PLACEMENT_DEPTH {
        parent = add(&mut doc, &format!("L{i}"), parent).0;
    }
    let all = listed(&doc, view(View::AllLevels)).unwrap();
    assert_eq!(all.rows_deep().last().map(|(item, _)| item.len()), Some(MAX_PLACEMENT_DEPTH));
    add(&mut doc, "Too deep", parent);
    assert!(listed(&doc, view(View::AllLevels)).is_err(), "65 levels");
}

/// Two parts with one part number are one row only when what they hold is the same all the way
/// down: here they differ two levels below, so in all levels they are two rows, each with its own.
#[test]
fn kits_are_compared_to_any_depth() {
    let mut doc = Document::default();
    let (a, _) = add(&mut doc, "A", 0);
    let (b, _) = add(&mut doc, "B", 0);
    let (x1, _) = add(&mut doc, "X1", a);
    let (x2, _) = add(&mut doc, "X2", b);
    let (bolt, _) = add(&mut doc, "Bolt", x1);
    for (id, part_number) in [(a, "P"), (b, "P"), (x1, "X"), (x2, "X"), (bolt, "BOLT")] {
        number(&mut doc, id, part_number);
    }
    let all = listed(&doc, view(View::AllLevels)).unwrap();
    assert_eq!(
        rows(&all),
        [
            row("1", "A", Normal, 1, 1),
            row("1.1", "X1", Normal, 1, 1),
            row("1.1.1", "Bolt", Normal, 1, 1),
            row("2", "B", Normal, 1, 1),
            row("2.1", "X2", Normal, 1, 1)
        ],
        "two P rows, never one with children at each 0"
    );
    // The same contents two levels down: one row again.
    let (bolt2, _) = add(&mut doc, "Bolt 2", x2);
    number(&mut doc, bolt2, "BOLT");
    let all = listed(&doc, view(View::AllLevels)).unwrap();
    assert_eq!(rows(&all), [row("1", "A; B", Normal, 2, 2), row("1.1", "X1; X2", Normal, 2, 1), row("1.1.1", "Bolt; Bolt 2", Normal, 2, 1)]);
}

/// What a folded part holds is listed as parts only lists it, in parts only and the first level,
/// and isn't folded again: a numbered subassembly inside it is listed through its parts. In all
/// levels it is listed in all levels again, so that subassembly is a row with its own rows.
#[test]
fn what_a_folded_part_holds_is_listed_as_parts_only_lists_it() {
    let mut doc = Document::default();
    let (machine, _) = add(&mut doc, "Machine", 0);
    let (gearbox, _) = add(&mut doc, "Gearbox", machine);
    let (shaft, _) = add(&mut doc, "Shaft", gearbox);
    place(&mut doc, shaft, gearbox);
    add(&mut doc, "Pin", gearbox);
    for (id, part_number) in [(machine, "M-1"), (gearbox, "GB-1"), (shaft, "SH-1")] {
        number(&mut doc, id, part_number);
    }
    for v in [View::PartsOnly, View::FirstLevel] {
        let bom = listed(&doc, folded(v)).unwrap();
        assert_eq!(rows(&bom), [row("1", "Machine", Normal, 1, 1), row("1.1", "Shaft", Normal, 2, 2), row("1.2", "Pin", Normal, 1, 1)], "{v:?}");
    }
    let all = listed(&doc, folded(View::AllLevels)).unwrap();
    let nested = [
        row("1", "Machine", Normal, 1, 1),
        row("1.1", "Gearbox", Normal, 1, 1),
        row("1.1.1", "Shaft", Normal, 2, 2),
        row("1.1.2", "Pin", Normal, 1, 1),
    ];
    assert_eq!(rows(&all), nested);
}

/// Bolt (purchased) holding a nut; Frame (inseparable) holding a plate, a purchased clip and a
/// weld kit (phantom) of a rivet and a purchased insert; Fixture (reference) holding a pin; Set
/// (phantom) holding a screw; Sub (normal, numbered, no bodies) holding a washer. Each is placed
/// once.
fn mixed() -> (Document, BTreeMap<&'static str, u64>) {
    let mut doc = Document::default();
    let mut at = BTreeMap::new();
    for (name, parent, bom_type) in [
        ("Bolt", "", Purchased),
        ("Nut", "Bolt", Normal),
        ("Frame", "", Inseparable),
        ("Plate", "Frame", Normal),
        ("Clip", "Frame", Purchased),
        ("Weld kit", "Frame", Phantom),
        ("Rivet", "Weld kit", Normal),
        ("Insert", "Weld kit", Purchased),
        ("Fixture", "", Reference),
        ("Pin", "Fixture", Normal),
        ("Set", "", Phantom),
        ("Screw", "Set", Normal),
        ("Sub", "", Normal),
        ("Washer", "Sub", Normal),
    ] {
        let parent = doc.find_component(parent).unwrap_or(0);
        let (id, occurrence) = add(&mut doc, name, parent);
        typed(&mut doc, id, bom_type);
        at.insert(name, occurrence);
    }
    let sub = doc.find_component("Sub").unwrap();
    number(&mut doc, sub, "SUB-1");
    (doc, at)
}

#[test]
fn row_of_finds_the_row_a_placement_is_counted_in() {
    let (doc, at) = mixed();
    let path = |names: &[&str]| -> Vec<u64> { names.iter().map(|n| at[*n]).collect() };
    let state = ModelState::default();
    let mut tree = Tree::new(&doc, &state);
    let name = |r: std::result::Result<&Row, Unlisted>| r.map(|r| r.names.join("; "));
    let flat = list_with(&mut tree, &Request::default(), true).unwrap();
    // Made for its rows alone, a list keeps no members.
    assert!(list(&mut tree, &Request::default()).unwrap().rows.iter().all(|r| r.members.is_empty()));
    let cases: &[(&[&str], std::result::Result<&str, Unlisted>)] = &[
        (&["Bolt"], Ok("Bolt")),
        (&["Bolt", "Nut"], Ok("Bolt")),
        (&["Frame"], Ok("Frame")),
        (&["Frame", "Plate"], Ok("Frame")),
        (&["Frame", "Clip"], Ok("Clip")),
        (&["Frame", "Weld kit"], Ok("Frame")),
        (&["Frame", "Weld kit", "Rivet"], Ok("Frame")),
        (&["Frame", "Weld kit", "Insert"], Ok("Insert")),
        (&["Fixture"], Err(Unlisted::Reference)),
        (&["Fixture", "Pin"], Err(Unlisted::Reference)),
        (&["Set"], Err(Unlisted::NoRow)),
        (&["Set", "Screw"], Ok("Screw")),
        (&["Sub"], Err(Unlisted::NoRow)),
        (&["Sub", "Washer"], Ok("Washer")),
        (&[], Err(Unlisted::NoRow)),
        (&["Nut"], Err(Unlisted::NotFound)),
    ];
    for (names, found) in cases {
        assert_eq!(name(flat.row_of(&tree, &path(names))), found.map(String::from), "{names:?}");
    }
    assert_eq!(flat.row_of(&tree, &[999]).err(), Some(Unlisted::NotFound));
    assert_eq!(flat.row_of(&tree, &vec![at["Bolt"]; 65]).err(), Some(Unlisted::NotFound));
    let stands: Vec<(String, Holds)> = flat.rows.iter().map(|r| (r.names.join("; "), r.stands_for)).collect();
    let want = [
        ("Bolt", Holds::All),
        ("Clip", Holds::All),
        ("Frame", Holds::AllButPurchased),
        ("Insert", Holds::All),
        ("Screw", Holds::Own),
        ("Washer", Holds::Own),
    ];
    assert_eq!(stands, want.map(|(n, h)| (n.to_string(), h)));

    let top = list_with(&mut tree, &view(View::FirstLevel), true).unwrap();
    let cases: &[(&[&str], &str)] = &[(&["Sub", "Washer"], "Sub"), (&["Set", "Screw"], "Screw"), (&["Frame", "Clip"], "Frame")];
    for (names, found) in cases {
        assert_eq!(name(top.row_of(&tree, &path(names))), Ok(found.to_string()), "{names:?}");
    }
    assert!(top.rows.iter().all(|r| r.stands_for == Holds::All));

    let kit = list_with(&mut tree, &folded(View::PartsOnly), true).unwrap();
    let sub = kit.row_of(&tree, &path(&["Sub"])).unwrap();
    let washer = kit.row_of(&tree, &path(&["Sub", "Washer"])).unwrap();
    assert_eq!((sub.names.join(""), sub.stands_for, washer.names.join("")), ("Sub".into(), Holds::All, "Washer".into()));
    assert_eq!(washer.key.parents, [sub.key.member]);
}

/// Each placement is counted in one row or none, every view and grouping: the counts, the
/// members and the total agree, and `row_of` finds each member's row.
#[test]
fn every_placement_is_in_one_row_or_none() {
    let (mut doc, _) = mixed();
    // More of the same: the subassembly and the bolt placed again, the washer twice in the subassembly.
    for name in ["Sub", "Bolt"] {
        let id = doc.find_component(name).unwrap();
        place(&mut doc, id, 0);
    }
    let (washer, sub) = (doc.find_component("Washer").unwrap(), doc.find_component("Sub").unwrap());
    place(&mut doc, washer, sub);
    let state = ModelState::default();
    let mut tree = Tree::new(&doc, &state);
    for v in View::ALL {
        for fold in [false, true] {
            for group in Group::ALL {
                let request = Request { view: v, fold, group, ..Request::default() };
                let bom = list_with(&mut tree, &request, true).unwrap();
                let deep = bom.rows_deep();
                let mut seen: Vec<&Vec<u64>> = deep.iter().flat_map(|(_, r)| &r.members).collect();
                assert_eq!(seen.len() as u64, bom.total.occurrences, "{request:?}");
                assert!(deep.iter().all(|(_, r)| r.count == r.members.len() as u64), "{request:?}");
                seen.sort();
                seen.dedup();
                assert_eq!(seen.len() as u64, bom.total.occurrences, "a placement in two rows: {request:?}");
                for (_, r) in &deep {
                    for m in &r.members {
                        assert_eq!(bom.row_of(&tree, m).map(|found| &found.key), Ok(&r.key), "{request:?} {m:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn a_damaged_files_structure_is_listed_or_refused_never_followed_forever() {
    let mut doc = Document::default();
    let (a, _) = add(&mut doc, "A", 0);
    let ghost =
        |id, component, parent| Occurrence { id, component, parent, name: format!("{id}"), transform: IDENTITY, grounded: false, bom_type: None };
    // An occurrence of a component that isn't there has no row and counts nowhere.
    doc.occurrences.push(ghost(900, 999, 0));
    assert_eq!(rows(&listed(&doc, Request::default()).unwrap()), [row("1", "A", Normal, 1, 1)]);
    // A component placed inside itself: refused at the depth limit, never walked forever.
    doc.occurrences.push(ghost(901, a, a));
    assert!(listed(&doc, Request::default()).is_err());
    // At the first level A is one row, but its material and its mass are what it holds: both walk inside.
    assert!(listed(&doc, Request { mass: true, ..view(View::FirstLevel) }).is_err(), "weighing it walks inside");
    assert!(listed(&doc, view(View::FirstLevel)).is_err(), "naming its material walks inside");
}

/// Moved from the engine's tests with the rule (it was `row_mass`), now with the reason.
#[test]
fn a_row_has_a_mass_only_when_its_definitions_are_weighed_and_agree() {
    let (steel, aluminium, surface, empty): (Weighed, Weighed, Weighed, Weighed) =
        (Some((392.5, 1)), Some((6.48, 1)), Some((0.0, 1)), Some((0.0, 0)));
    assert_eq!(agree(&[steel, steel]), Mass::Grams(392.5));
    assert_eq!(agree(&[steel, aluminium]), Mass::Unknown(Unknown::Differs));
    assert_eq!(agree(&[surface, surface]), Mass::Grams(0.0), "a body that weighs nothing (a surface) is weighed");
    assert_eq!(agree(&[surface, steel]), Mass::Unknown(Unknown::Differs));
    assert_eq!(agree(&[empty]), Mass::Unknown(Unknown::NothingToWeigh), "nothing to weigh");
    assert_eq!(agree(&[steel, empty]), Mass::Unknown(Unknown::Differs), "one weighs something, one nothing");
    assert_eq!(agree(&[empty, steel]), Mass::Unknown(Unknown::Differs), "whichever comes first");
    assert_eq!(agree(&[empty, empty]), Mass::Unknown(Unknown::NothingToWeigh));
    assert_eq!(agree(&[None]), Mass::Unknown(Unknown::Unmeasurable), "not measured");
    assert_eq!(agree(&[aluminium, None, steel]), Mass::Unknown(Unknown::Unmeasurable), "whichever definition it is");
    assert_eq!(agree(&[]), Mass::Unknown(Unknown::NothingToWeigh));
}

/// What a row's part stands for leaves out a reference, and an inseparable part's purchased parts,
/// which its list shows beside it; phantoms and inseparable parts inside are weighed through.
#[test]
fn what_adds_to_a_row_follows_what_it_stands_for() {
    for t in BomType::ALL {
        assert!(!adds(Holds::Own, t), "{t:?}");
        assert_eq!(adds(Holds::All, t), t != Reference, "{t:?}");
        assert_eq!(adds(Holds::AllButPurchased, t), !matches!(t, Reference | Purchased), "{t:?}");
    }
}

/// `how` applied to the view `request` lists, its numbering saved as the engine saves it; whether
/// anything changed.
fn numbered(doc: &mut Document, request: Request, how: Numbering) -> bool {
    let state = ModelState::default();
    let Some(numbers) = super::number(&mut Tree::new(doc, &state), &request, how).unwrap() else { return false };
    let saved = &mut doc.assembly.item_numbers;
    saved.retain(|n| (n.source, n.view) != (numbers.source, numbers.view));
    saved.push(numbers);
    true
}

/// [`record`] as the engine runs it after a change; whether anything changed.
fn recorded(doc: &mut Document) -> bool {
    let state = ModelState::default();
    let Some(numbers) = record(&mut Tree::new(doc, &state)) else { return false };
    doc.assembly.item_numbers = numbers;
    true
}

const RENUMBER: Numbering = Numbering::Renumber { start: 1, step: 1, order: Order::List };

/// Each row, followed by the rows under it: item, names, and its numbering flags as letters
/// (provisional, locked, duplicate).
fn items(bom: &Bom) -> Vec<(String, String, String)> {
    bom.rows_deep()
        .into_iter()
        .map(|(item, r)| {
            let flags = [(r.provisional, 'p'), (r.locked, 'l'), (r.duplicate, 'd')].iter().filter(|(on, _)| *on).map(|(_, c)| *c).collect();
            (item_text(&item), r.names.join("; "), flags)
        })
        .collect()
}

fn item(item: &str, names: &str, flags: &str) -> (String, String, String) {
    (item.into(), names.into(), flags.into())
}

/// Parts A, B and C at the top, part numbers P1 to P3.
fn three_parts() -> (Document, [(u64, u64); 3]) {
    let mut doc = Document::default();
    let parts = ["A", "B", "C"].map(|name| add(&mut doc, name, 0));
    for (i, (part, _)) in parts.iter().enumerate() {
        number(&mut doc, *part, &format!("P{}", i + 1));
    }
    (doc, parts)
}

#[test]
fn numbering_a_view_saves_its_numbers_and_new_rows_take_the_next() {
    let (mut doc, [_, (b, ob), _]) = three_parts();
    // Never numbered: positions, provisional, and nothing saved by a change.
    let bom = listed(&doc, Request::default()).unwrap();
    assert!(!bom.numbered);
    assert_eq!(items(&bom), [item("1", "A", "p"), item("2", "B", "p"), item("3", "C", "p")]);
    assert!(!recorded(&mut doc), "a design never numbered saves nothing");
    assert!(numbered(&mut doc, Request::default(), RENUMBER));
    assert!(!numbered(&mut doc, Request::default(), RENUMBER), "the same numbers again change nothing");
    let bom = listed(&doc, Request::default()).unwrap();
    assert!(bom.numbered);
    assert_eq!(items(&bom), [item("1", "A", ""), item("2", "B", ""), item("3", "C", "")]);
    // A new part, first by part number: the highest number + 1, listed by it, saved by the next change.
    let (d, _) = add(&mut doc, "D", 0);
    number(&mut doc, d, "P0");
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()).last(), Some(&item("4", "D", "p")));
    assert!(recorded(&mut doc));
    assert!(!recorded(&mut doc), "saved once");
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()).last(), Some(&item("4", "D", "")));
    // B removed leaves a gap, and back it has its number again.
    doc.occurrences.retain(|o| o.id != ob);
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), [item("1", "A", ""), item("3", "C", ""), item("4", "D", "")]);
    assert!(!recorded(&mut doc), "a gap isn't filled");
    // Setting another row's number keeps B's: it isn't listed, but it is saved.
    assert!(numbered(&mut doc, Request::default(), Numbering::Set { row: vec![1], item: None, locked: Some(true) }));
    assert!(numbered(&mut doc, Request::default(), Numbering::Set { row: vec![1], item: None, locked: Some(false) }));
    let (e, _) = add(&mut doc, "E", 0);
    assert!(recorded(&mut doc));
    place(&mut doc, b, 0);
    let bom = listed(&doc, Request::default()).unwrap();
    assert_eq!(items(&bom), [item("1", "A", ""), item("2", "B", ""), item("3", "C", ""), item("4", "D", ""), item("5", "E", "")]);
    assert_eq!(bom.rows.last().map(|r| r.key.member.component), Some(e));
    // A renumber closing the gaps in item order, or in the list's order (by part number).
    doc.occurrences.retain(|o| o.component != b);
    assert!(numbered(&mut doc, Request::default(), Numbering::Renumber { start: 1, step: 1, order: Order::Item }));
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), [item("1", "A", ""), item("2", "C", ""), item("3", "D", ""), item("4", "E", "")]);
    assert!(numbered(&mut doc, Request::default(), Numbering::Renumber { start: 10, step: 5, order: Order::List }));
    assert_eq!(
        items(&listed(&doc, Request::default()).unwrap()),
        [item("10", "D", ""), item("15", "A", ""), item("20", "C", ""), item("25", "E", "")]
    );
    // The renumber forgot B: back, it is new.
    place(&mut doc, b, 0);
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()).last(), Some(&item("26", "B", "p")));
}

/// A view numbered twice, which only a damaged file has: listing reads the first numbering, and
/// recording drops the second, so a change never lists the view again for it.
#[test]
fn a_view_numbered_twice_keeps_its_first_numbering() {
    let (mut doc, _) = three_parts();
    assert!(numbered(&mut doc, Request::default(), RENUMBER));
    let mut second = doc.assembly.item_numbers[0].clone();
    second.items.iter_mut().for_each(|i| i.number += 10);
    doc.assembly.item_numbers.push(second);
    let first = [item("1", "A", ""), item("2", "B", ""), item("3", "C", "")];
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), first);
    assert!(recorded(&mut doc));
    assert_eq!(doc.assembly.item_numbers.len(), 1);
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), first);
    assert!(!recorded(&mut doc));
}

#[test]
fn a_row_keeps_its_number_through_a_part_number_edit() {
    let (mut doc, [(a, _), _, _]) = three_parts();
    assert!(numbered(&mut doc, Request::default(), RENUMBER));
    number(&mut doc, a, "P9");
    assert!(!recorded(&mut doc), "no new row");
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), [item("1", "A", ""), item("2", "B", ""), item("3", "C", "")]);
}

/// The defect the plan names: a renumber with locked rows never gives two rows one number.
#[test]
fn a_renumber_keeps_locked_numbers_and_never_gives_two_rows_one() {
    let (mut doc, _) = three_parts();
    let (d, _) = add(&mut doc, "D", 0);
    number(&mut doc, d, "P4");
    // Setting a number numbers the view first; one another row has is marked on both.
    assert!(numbered(&mut doc, Request::default(), Numbering::Set { row: vec![3], item: Some(2), locked: Some(true) }));
    assert_eq!(
        items(&listed(&doc, Request::default()).unwrap()),
        [item("1", "A", ""), item("2", "B", "d"), item("2", "C", "ld"), item("4", "D", "")]
    );
    for (start, step, want) in [(1, 1, ["1", "3", "2", "4"]), (2, 1, ["3", "4", "2", "5"]), (1, 2, ["1", "3", "2", "5"])] {
        assert!(numbered(&mut doc, Request::default(), Numbering::Renumber { start, step, order: Order::List }));
        let bom = listed(&doc, Request::default()).unwrap();
        let by_name: BTreeMap<String, (String, String)> = items(&bom).into_iter().map(|(i, n, f)| (n, (i, f))).collect();
        let got: Vec<&str> = ["A", "B", "C", "D"].iter().map(|n| by_name[*n].0.as_str()).collect();
        assert_eq!(got, want, "from {start} by {step}");
        assert!(bom.rows.iter().all(|r| !r.duplicate), "from {start} by {step}");
        assert_eq!(by_name["C"].1, "l");
    }
    // Unlocked, the next renumber moves it.
    assert!(numbered(&mut doc, Request::default(), Numbering::Set { row: vec![2], item: None, locked: Some(false) }));
    assert!(numbered(&mut doc, Request::default(), RENUMBER));
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), [item("1", "A", ""), item("2", "B", ""), item("3", "C", ""), item("4", "D", "")]);
}

#[test]
fn all_levels_are_numbered_within_each_parent() {
    let mut doc = Document::default();
    let (frame, _) = add(&mut doc, "Frame", 0);
    let (bolt, _) = add(&mut doc, "Bolt", 0);
    number(&mut doc, frame, "F");
    number(&mut doc, bolt, "B");
    let (beam, _) = add(&mut doc, "Beam", frame);
    number(&mut doc, beam, "F1");
    let all = view(View::AllLevels);
    assert!(numbered(&mut doc, all, RENUMBER));
    assert_eq!(items(&listed(&doc, all).unwrap()), [item("1", "Bolt", ""), item("2", "Frame", ""), item("2.1", "Beam", "")]);
    // A plate in the frame is the frame's next; the design's parts-only view isn't numbered.
    let (plate, _) = add(&mut doc, "Plate", frame);
    number(&mut doc, plate, "F0");
    assert_eq!(items(&listed(&doc, all).unwrap()).last(), Some(&item("2.2", "Plate", "p")));
    assert!(recorded(&mut doc));
    assert_eq!(items(&listed(&doc, all).unwrap()).last(), Some(&item("2.2", "Plate", "")));
    assert!(!listed(&doc, Request::default()).unwrap().numbered);
    // Renumbered in the list's order, each level from the start.
    assert!(numbered(&mut doc, all, Numbering::Renumber { start: 5, step: 5, order: Order::List }));
    assert_eq!(
        items(&listed(&doc, all).unwrap()),
        [item("5", "Bolt", ""), item("10", "Frame", ""), item("10.5", "Plate", ""), item("10.10", "Beam", "")]
    );
    // A component's own list is numbered apart from the design's.
    let own = Request { source: frame, ..all };
    assert!(!listed(&doc, own).unwrap().numbered);
    assert!(numbered(&mut doc, own, RENUMBER));
    assert_eq!(items(&listed(&doc, own).unwrap()), [item("1", "Plate", ""), item("2", "Beam", "")]);
    assert_eq!(items(&listed(&doc, all).unwrap())[2], item("10.5", "Plate", ""));
}

/// A numbering keeps the list options it was made with: the next change saves the rows they list.
/// A list with other options reads the same numbers by row, the rest provisional.
#[test]
fn a_numbering_keeps_its_list_options_and_other_lists_read_its_numbers() {
    let (mut doc, [_, (b, _), _]) = three_parts();
    number(&mut doc, b, "P1");
    let by_definition = Request { group: Group::Definition, ..Request::default() };
    assert!(numbered(&mut doc, Request::default(), RENUMBER));
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), [item("1", "A; B", ""), item("2", "C", "")]);
    assert_eq!(items(&listed(&doc, by_definition).unwrap()), [item("1", "A", ""), item("2", "C", ""), item("3", "B", "p")]);
    assert!(!recorded(&mut doc), "the numbering lists by part number: nothing new");
    assert!(numbered(&mut doc, by_definition, Numbering::Set { row: vec![3], item: Some(7), locked: None }));
    let saved = &doc.assembly.item_numbers;
    assert_eq!((saved.len(), saved[0].group), (1, Group::Definition), "one numbering a view, with the options last used");
    assert_eq!(items(&listed(&doc, by_definition).unwrap()), [item("1", "A", ""), item("2", "C", ""), item("7", "B", "")]);
}

#[test]
fn numbers_are_refused_outside_their_range_and_rows_are_found_as_shown() {
    let (mut doc, _) = three_parts();
    let refused = |doc: &mut Document, how: Numbering| {
        let state = ModelState::default();
        super::number(&mut Tree::new(doc, &state), &Request::default(), how).unwrap_err().to_string()
    };
    for (start, step) in [(0, 1), (1, 0), (MAX_ITEM + 1, 1)] {
        assert!(refused(&mut doc, Numbering::Renumber { start, step, order: Order::List }).contains("1 to 999999"), "{start} by {step}");
    }
    assert!(refused(&mut doc, Numbering::Renumber { start: MAX_ITEM - 1, step: 1, order: Order::List }).contains("passes 999999"));
    assert!(refused(&mut doc, Numbering::Renumber { start: 1, step: MAX_ITEM, order: Order::List }).contains("passes 999999"));
    assert!(refused(&mut doc, Numbering::Set { row: vec![4], item: Some(1), locked: None }).contains("item 4"));
    assert!(refused(&mut doc, Numbering::Set { row: vec![1, 1], item: Some(1), locked: None }).contains("item 1.1"));
    assert!(refused(&mut doc, Numbering::Set { row: vec![1], item: Some(0), locked: None }).contains("1 to 999999"));
    assert!(doc.assembly.item_numbers.is_empty(), "nothing saved");
    // The highest number is taken; a renumber that would pass it is refused whole.
    assert!(numbered(&mut doc, Request::default(), Numbering::Renumber { start: MAX_ITEM - 2, step: 1, order: Order::List }));
    assert_eq!(listed(&doc, Request::default()).unwrap().rows.last().map(|r| r.item), Some(MAX_ITEM));
}

/// A damaged file's numbers: one out of range is no number, and of a key saved twice the first
/// holds. A component's numbering goes with it.
#[test]
fn saved_numbers_are_read_leniently_and_go_with_their_component() {
    let (mut doc, [(a, _), (b, _), (c, _)]) = three_parts();
    let key = |component| RowKey { parents: vec![], member: Member { component, bom_type: Normal } };
    let saved = |key, number| ItemNumber { key, number, locked: false };
    doc.assembly.item_numbers.push(ItemNumbers {
        source: 0,
        view: View::PartsOnly,
        group: Group::PartNumber,
        fold: false,
        purchased_contents: false,
        items: vec![saved(key(a), 4), saved(key(a), 9), saved(key(b), 0), saved(key(c), MAX_ITEM + 1)],
    });
    assert_eq!(items(&listed(&doc, Request::default()).unwrap()), [item("4", "A", ""), item("5", "B", "p"), item("6", "C", "p")]);
    let (sub, _) = add(&mut doc, "Sub", 0);
    add(&mut doc, "Inner", sub);
    assert!(numbered(&mut doc, Request { source: sub, ..Request::default() }, RENUMBER));
    assert_eq!(doc.assembly.item_numbers.len(), 2);
    doc.components.retain(|c| c.id != sub);
    doc.occurrences.retain(|o| o.component != sub && o.parent != sub);
    assert!(recorded(&mut doc));
    assert_eq!(doc.assembly.item_numbers.iter().map(|n| n.source).collect::<Vec<_>>(), [0], "the design's is kept");
}

/// Numbers are saved in the design under `assembly.item_numbers`, and change no geometry.
#[test]
fn item_numbers_are_saved_and_are_part_properties() {
    let (mut doc, _) = three_parts();
    let before = doc.clone();
    assert!(numbered(&mut doc, Request { fold: true, ..view(View::AllLevels) }, RENUMBER));
    assert!(doc.differs_only_in_part_properties(&before));
    assert!(!doc.differs_only_in_part_properties(&doc.clone()), "no change is no part-properties change");
    let json = serde_json::to_value(&doc.assembly).unwrap();
    let first = &json["item_numbers"][0];
    assert_eq!(
        (&first["source"], &first["levels"], &first["group_by"], &first["group_children"]),
        (&json!(0), &json!("all"), &json!("part_number"), &json!(true))
    );
    assert_eq!(first["items"][0], json!({"member": {"component": doc.components[0].id, "bom_type": "normal"}, "number": 1}));
    // Through the whole design, as a save writes it: a design whose assembly holds only numbers keeps them.
    assert_eq!(Document::from_json(&doc.to_json()).unwrap().assembly, doc.assembly);
    assert!(serde_json::to_value(&before.assembly).unwrap().get("item_numbers").is_none(), "a design never numbered saves nothing new");
}
