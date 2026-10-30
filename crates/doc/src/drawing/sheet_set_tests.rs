use std::sync::Arc;

use solidcraft_geom::drawing_sheet::{Orientation, PaperSize};

use super::super::{DEFAULT_DPI, Excluded, MAX_NAME_BYTES, MAX_SHEETS, Sheet};
use super::*;

/// The seed the test drawings' ids come from.
const SEED: u64 = 9723;

/// A drawing of `names.len()` sheets named `names`, in that order, its ids from [`SEED`].
fn drawing(names: &[&str]) -> Drawing {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = || solidcraft_geom::drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let first = names.first().copied().unwrap_or("Sheet 1");
    let mut d = Drawing::new("Gearbox", "en-GB", first, a3(), None, &BTreeSet::new(), &mut id_source).unwrap();
    for name in names.iter().skip(1) {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.sheets.push(Arc::new(Sheet {
            id,
            name: (*name).into(),
            sheet: a3(),
            border: None,
            dpi: DEFAULT_DPI,
            documents: None,
            excluded: Excluded::default(),
        }));
    }
    d
}

/// Four sheets: Assembly, Housing, Shaft, Notes.
fn gearbox() -> Drawing {
    drawing(&["Assembly", "Housing", "Shaft", "Notes"])
}

fn id_of(d: &Drawing, name: &str) -> u64 {
    d.sheets[d.sheet_index(name).unwrap()].id
}

fn names_at(d: &Drawing, positions: &[usize]) -> Vec<String> {
    positions.iter().map(|&i| d.sheets[i].name.clone()).collect()
}

fn err(r: Result<impl std::fmt::Debug>) -> String {
    r.unwrap_err().to_string()
}

#[test]
fn a_set_selects_sheets_by_id_or_name_and_keeps_them_by_id() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    let shaft = ids::encode(id_of(&d, "Shaft"));
    let id = sets.add(&d, "Workshop", &["Housing", &shaft], &mut IdSource::seeded(1)).unwrap();
    let set = sets.get("Workshop").unwrap();
    assert_eq!(set.id, id);
    let mut want = vec![id_of(&d, "Housing"), id_of(&d, "Shaft")];
    want.sort_unstable();
    assert_eq!(set.sheets(), want.as_slice());
    assert!(sets.get(&ids::encode(id)).is_ok(), "named by its id too");
    sets.validate(&d).unwrap();
}

#[test]
fn a_new_set_s_id_differs_from_every_id_of_the_drawing_and_of_the_other_sets() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    // The drawing's own seed: its first draws are the drawing's ids, which are passed over.
    let mut id_source = IdSource::seeded(SEED);
    let drawing_ids: BTreeSet<u64> = d.ids().collect();
    for n in 0..8 {
        let id = sets.add(&d, &format!("Set {n}"), &["Assembly"], &mut id_source).unwrap();
        assert!(id != 0 && !drawing_ids.contains(&id));
    }
    let set_ids: BTreeSet<u64> = sets.sets().iter().map(|s| s.id).collect();
    assert_eq!(set_ids.len(), 8);
    assert!(sets.sets().is_sorted_by_key(|s| s.id), "listed by id");
    sets.validate(&d).unwrap();
}

#[test]
fn the_same_seed_gives_the_same_sets() {
    let d = gearbox();
    let make = || {
        let mut sets = SheetSets::default();
        let mut id_source = IdSource::seeded(7);
        sets.add(&d, "A", &["Shaft", "Assembly"], &mut id_source).unwrap();
        sets.add(&d, "B", &["Notes"], &mut id_source).unwrap();
        sets
    };
    assert_eq!(make(), make());
}

#[test]
fn a_set_comes_out_in_the_drawing_s_order_whatever_order_it_was_given_in() {
    let mut d = gearbox();
    let mut sets = SheetSets::default();
    sets.add(&d, "Parts", &["Shaft", "Housing", "Assembly"], &mut IdSource::seeded(1)).unwrap();
    let out = sets.resolve(&d, &Scope::Set("Parts")).unwrap();
    assert_eq!(names_at(&d, &out), ["Assembly", "Housing", "Shaft"]);
    // Reordering the drawing reorders the set's output; the set itself doesn't change.
    let before = sets.clone();
    d.sheets.reverse();
    let out = sets.resolve(&d, &Scope::Set("Parts")).unwrap();
    assert_eq!(names_at(&d, &out), ["Shaft", "Housing", "Assembly"]);
    assert_eq!(sets, before);
    sets.validate(&d).unwrap();
}

#[test]
fn two_sets_may_share_a_name_and_are_then_named_by_id() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    let a = sets.add(&d, "Customer", &["Assembly"], &mut id_source).unwrap();
    let b = sets.add(&d, "Customer", &["Housing"], &mut id_source).unwrap();
    sets.validate(&d).unwrap();
    assert!(err(sets.index("Customer")).contains("more than one sheet set is called `Customer`: give its id"));
    assert_eq!(sets.get(&ids::encode(a)).unwrap().sheets(), [id_of(&d, "Assembly")]);
    assert_eq!(sets.get(&ids::encode(b)).unwrap().sheets(), [id_of(&d, "Housing")]);
    // Renaming one makes both names unique again.
    assert!(sets.rename(&ids::encode(b), "Supplier").unwrap());
    assert_eq!(sets.get("Customer").unwrap().id, a);
}

#[test]
fn a_set_is_named_by_its_own_id_never_by_a_sheet_s() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    sets.add(&d, "Workshop", &["Housing"], &mut IdSource::seeded(1)).unwrap();
    let sheet = ids::encode(id_of(&d, "Housing"));
    assert!(err(sets.index(&sheet)).contains(&format!("no sheet set `{sheet}`")));
    assert!(err(sets.index("Nope")).contains("no sheet set `Nope`"));
    assert!(err(SheetSets::default().get("Workshop")).contains("no sheet set `Workshop`"));
}

#[test]
fn a_set_s_name_keeps_the_drawing_s_name_rule() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    let longest = "é".repeat(MAX_NAME_BYTES / 2);
    assert_eq!(longest.len(), MAX_NAME_BYTES);
    sets.add(&d, &longest, &["Assembly"], &mut id_source).unwrap();
    let too_long = format!("{longest}x");
    for bad in ["", "   ", "Tab\there", "Line\nbreak", "\u{7f}", too_long.as_str()] {
        let why = err(sets.add(&d, bad, &["Assembly"], &mut id_source));
        assert!(why.contains("a sheet set's name is 1 to 256 bytes"), "{bad:?}: {why}");
        assert!(err(sets.rename(&longest, bad)).contains("name is 1 to 256 bytes"), "{bad:?}");
    }
    assert_eq!(sets.len(), 1, "nothing added");
    assert_eq!(sets.sets()[0].name, longest, "nothing renamed");
}

#[test]
fn a_set_s_sheets_are_one_or_more_named_once_each_all_of_the_drawing() {
    let d = drawing(&["Assembly", "Detail", "Detail", "Notes"]);
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    let assembly = ids::encode(id_of(&d, "Assembly"));
    let cases: [(&[&str], &str); 5] = [
        (&[], "name 1 to 1000 sheets"),
        (&["Assembly", "Assembly"], "sheet `Assembly` is named twice"),
        (&["Assembly", &assembly], "is named twice"),
        (&["Detail"], "more than one sheet is called `Detail`: give its id"),
        (&["Notes", "Missing"], "no sheet `Missing`"),
    ];
    for (sheets, want) in cases {
        let why = err(sets.add(&d, "Set", sheets, &mut id_source));
        assert!(why.contains(want), "{sheets:?}: {why}");
    }
    assert!(sets.is_empty());
    // Two sheets of one name are each named by id.
    let details: Vec<String> = d.sheets.iter().filter(|s| s.name == "Detail").map(|s| ids::encode(s.id)).collect();
    let named: Vec<&str> = details.iter().map(String::as_str).collect();
    sets.add(&d, "Details", &named, &mut id_source).unwrap();
    assert_eq!(sets.sets()[0].sheets().len(), 2);
    sets.validate(&d).unwrap();
}

#[test]
fn a_set_holds_every_sheet_of_the_largest_drawing_and_no_list_longer_is_read() {
    let names: Vec<String> = (0..MAX_SHEETS).map(|i| format!("Sheet {i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let d = drawing(&refs);
    assert_eq!(d.sheets.len(), MAX_SHEETS);
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    sets.add(&d, "All", &refs, &mut id_source).unwrap();
    sets.validate(&d).unwrap();
    let out = sets.resolve(&d, &Scope::Set("All")).unwrap();
    assert_eq!(out, (0..MAX_SHEETS).collect::<Vec<_>>());
    // A hostile list is refused by its length, before any name is looked up.
    let long = vec!["Sheet 0"; MAX_SHEETS + 1];
    assert!(err(sets.add(&d, "Too many", &long, &mut id_source)).contains("name 1 to 1000 sheets"));
    assert!(err(sets.resolve(&d, &Scope::Sheets(long))).contains("name 1 to 1000 sheets"));
}

#[test]
fn a_drawing_has_at_most_the_cap_of_sets() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    for n in 0..MAX_SHEET_SETS {
        sets.add(&d, &format!("Set {n}"), &["Notes"], &mut id_source).unwrap();
    }
    sets.validate(&d).unwrap();
    let why = err(sets.add(&d, "One more", &["Notes"], &mut id_source));
    assert!(why.contains("at most 256 sheet sets"), "{why}");
    assert_eq!(sets.len(), MAX_SHEET_SETS);
}

#[test]
fn renaming_says_whether_anything_changed() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    sets.add(&d, "Workshop", &["Shaft"], &mut IdSource::seeded(1)).unwrap();
    assert!(!sets.rename("Workshop", "Workshop").unwrap());
    assert!(sets.rename("Workshop", "Machining").unwrap());
    assert!(sets.get("Workshop").is_err());
    assert_eq!(sets.get("Machining").unwrap().sheets(), [id_of(&d, "Shaft")]);
    assert!(err(sets.rename("Workshop", "Other")).contains("no sheet set `Workshop`"));
    sets.validate(&d).unwrap();
}

#[test]
fn changing_a_set_s_sheets_says_whether_anything_changed_and_a_refusal_changes_nothing() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    sets.add(&d, "Parts", &["Housing", "Shaft"], &mut IdSource::seeded(1)).unwrap();
    let shaft = ids::encode(id_of(&d, "Shaft"));
    assert!(!sets.set_sheets(&d, "Parts", &[&shaft, "Housing"]).unwrap(), "the same sheets, named otherwise");
    let before = sets.clone();
    assert!(err(sets.set_sheets(&d, "Parts", &["Housing", "Gone"])).contains("no sheet `Gone`"));
    assert!(err(sets.set_sheets(&d, "Parts", &[])).contains("name 1 to 1000 sheets"));
    assert!(err(sets.set_sheets(&d, "Nope", &["Housing"])).contains("no sheet set `Nope`"));
    assert_eq!(sets, before);
    assert!(sets.set_sheets(&d, "Parts", &["Notes"]).unwrap());
    assert_eq!(sets.get("Parts").unwrap().sheets(), [id_of(&d, "Notes")]);
    sets.validate(&d).unwrap();
}

#[test]
fn removing_a_set_answers_it_and_leaves_the_others() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    sets.add(&d, "A", &["Assembly"], &mut id_source).unwrap();
    let b = sets.add(&d, "B", &["Housing"], &mut id_source).unwrap();
    let removed = sets.remove("A").unwrap();
    assert_eq!((removed.name.as_str(), removed.sheets()), ("A", &[id_of(&d, "Assembly")][..]));
    assert_eq!(sets.sets().iter().map(|s| s.id).collect::<Vec<_>>(), [b]);
    assert!(err(sets.remove("A")).contains("no sheet set `A`"));
    sets.validate(&d).unwrap();
}

#[test]
fn deleting_a_sheet_reports_its_sets_then_drops_it_and_any_set_it_empties() {
    let mut d = gearbox();
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    let both = sets.add(&d, "Both", &["Housing", "Shaft"], &mut id_source).unwrap();
    let only = sets.add(&d, "Only housing", &["Housing"], &mut id_source).unwrap();
    let other = sets.add(&d, "Notes", &["Notes"], &mut id_source).unwrap();
    let housing = id_of(&d, "Housing");
    let mut holding = vec![both, only];
    holding.sort_unstable();
    assert_eq!(sets.holding(housing), holding);
    assert!(sets.holding(id_of(&d, "Assembly")).is_empty());

    d.remove_sheet(housing);
    let removed = sets.without_sheet(housing);
    assert_eq!(removed, SheetsRemoved { shrunk: vec![both], emptied: vec![only] });
    assert_eq!(sets.get("Both").unwrap().sheets(), [id_of(&d, "Shaft")]);
    assert!(sets.get("Only housing").is_err());
    assert_eq!(sets.get("Notes").unwrap().id, other);
    sets.validate(&d).unwrap();
    // Nothing holds it any more: dropping it again changes nothing.
    assert_eq!(sets.without_sheet(housing), SheetsRemoved::default());
}

#[test]
fn every_sheet_is_every_sheet_not_left_out_of_printing() {
    let mut d = gearbox();
    let sets = SheetSets::default();
    let out = sets.resolve(&d, &Scope::Printed).unwrap();
    assert_eq!(names_at(&d, &out), ["Assembly", "Housing", "Shaft", "Notes"]);
    let notes = d.sheet_index("Notes").unwrap();
    Arc::make_mut(&mut d.sheets[notes]).excluded.print = true;
    // Left out of the sheet count is not left out of printing.
    Arc::make_mut(&mut d.sheets[0]).excluded.count = true;
    let out = sets.resolve(&d, &Scope::Printed).unwrap();
    assert_eq!(names_at(&d, &out), ["Assembly", "Housing", "Shaft"]);
    for sheet in &mut d.sheets {
        Arc::make_mut(sheet).excluded.print = true;
    }
    let why = err(sets.resolve(&d, &Scope::Printed));
    assert!(why.contains("every sheet is left out of printing: name the sheets to print"), "{why}");
}

#[test]
fn a_sheet_named_outright_comes_out_even_when_left_out_of_printing() {
    let mut d = gearbox();
    let mut sets = SheetSets::default();
    for sheet in &mut d.sheets {
        Arc::make_mut(sheet).excluded.print = true;
    }
    assert_eq!(names_at(&d, &sets.resolve(&d, &Scope::Sheet("Shaft")).unwrap()), ["Shaft"]);
    let list = sets.resolve(&d, &Scope::Sheets(vec!["Notes", "Housing"])).unwrap();
    assert_eq!(names_at(&d, &list), ["Housing", "Notes"], "in the drawing's order");
    sets.add(&d, "Parts", &["Shaft", "Housing"], &mut IdSource::seeded(1)).unwrap();
    assert_eq!(names_at(&d, &sets.resolve(&d, &Scope::Set("Parts")).unwrap()), ["Housing", "Shaft"]);
}

#[test]
fn a_scope_naming_what_isn_t_there_is_refused() {
    let mut d = gearbox();
    let mut sets = SheetSets::default();
    sets.add(&d, "Parts", &["Shaft", "Housing"], &mut IdSource::seeded(1)).unwrap();
    assert!(err(sets.resolve(&d, &Scope::Sheet("Gone"))).contains("no sheet `Gone`"));
    assert!(err(sets.resolve(&d, &Scope::Sheets(vec!["Shaft", "Shaft"]))).contains("named twice"));
    assert!(err(sets.resolve(&d, &Scope::Sheets(Vec::new()))).contains("name 1 to 1000 sheets"));
    assert!(err(sets.resolve(&d, &Scope::Set("Nope"))).contains("no sheet set `Nope`"));
    // A sheet deleted without dropping it from the sets: the set is stale, never printed short.
    let shaft = id_of(&d, "Shaft");
    d.remove_sheet(shaft);
    let why = err(sets.resolve(&d, &Scope::Set("Parts")));
    assert!(why.contains(&format!("sheet set `Parts` holds sheet {}, which the drawing doesn't have", ids::encode(shaft))), "{why}");
    assert!(sets.validate(&d).is_err());
}

#[test]
fn validation_refuses_every_planted_fault() {
    let d = gearbox();
    let mut good = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    good.add(&d, "A", &["Assembly", "Shaft"], &mut id_source).unwrap();
    good.add(&d, "B", &["Housing"], &mut id_source).unwrap();
    good.validate(&d).unwrap();
    let (first, second) = (good.sets[0].id, good.sets[1].id);
    let assembly = id_of(&d, "Assembly");
    let shaft = id_of(&d, "Shaft");
    let (low, high) = (assembly.min(shaft), assembly.max(shaft));

    type Plant = Box<dyn Fn(&mut SheetSets)>;
    let faults: Vec<(Plant, String)> = vec![
        (Box::new(|s| s.sets[0].id = 0), "has the id 0".into()),
        (Box::new(move |s| s.sets[1].id = first), format!("two sheet sets have the id {}", ids::encode(first))),
        (Box::new(move |s| s.sets[0].id = assembly), "has the id of an object of the drawing".into()),
        (Box::new(|s| s.sets.reverse()), "not listed by id".into()),
        (Box::new(|s| s.sets[0].name = " ".into()), "name is 1 to 256 bytes".into()),
        (Box::new(|s| s.sets[0].sheets.clear()), "holds 1 to 1000 sheets".into()),
        (Box::new(move |s| s.sets[0].sheets = vec![high, low]), "not listed by id, once each".into()),
        (Box::new(move |s| s.sets[0].sheets = vec![low, low]), "not listed by id, once each".into()),
        (Box::new(move |s| s.sets[0].sheets = vec![low, high, u64::MAX]), "which the drawing doesn't have".into()),
        (Box::new(move |s| s.sets[1].sheets = vec![second]), "which the drawing doesn't have".into()),
        (
            Box::new(|s| {
                let one = s.sets[1].clone();
                s.sets = (0..=MAX_SHEET_SETS as u64).map(|i| SheetSet { id: u64::MAX - i, ..one.clone() }).collect();
                s.sets.sort_by_key(|set| set.id);
            }),
            "at most 256 sheet sets".into(),
        ),
    ];
    for (plant, want) in faults {
        let mut bad = good.clone();
        plant(&mut bad);
        let why = err(bad.validate(&d));
        assert!(why.contains(&want), "want {want:?}, got {why}");
    }
}

#[test]
fn hostile_names_never_panic() {
    let d = gearbox();
    let mut sets = SheetSets::default();
    let mut id_source = IdSource::seeded(1);
    sets.add(&d, "Set", &["Assembly"], &mut id_source).unwrap();
    let huge = "x".repeat(1 << 20);
    for named in ["", "\0", "0000000000000", "zzzzzzzzzzzzz", "ÿÿÿ", "\u{202e}Assembly", huge.as_str()] {
        let _ = sets.index(named);
        let _ = sets.rename(named, "Other");
        let _ = sets.set_sheets(&d, named, &[named]);
        let _ = sets.resolve(&d, &Scope::Sheet(named));
        let _ = sets.resolve(&d, &Scope::Set(named));
        let _ = sets.add(&d, named, &[named], &mut id_source);
        let _ = sets.remove(named);
    }
    sets.validate(&d).unwrap();
}
