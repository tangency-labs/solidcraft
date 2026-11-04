use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};

use super::*;
use crate::drawing::{OnSheet, Source as DrawingSource};
use crate::ids::IdSource;

/// A drawing of one design on one A3 sheet with a parts list and a revision table, and the first's id.
fn tabled() -> (Drawing, u64) {
    let mut id_source = IdSource::seeded(3874);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Bolted", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let source = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(DrawingSource { id: source });
    let sheet = d.sheets[0].id;
    let list = Kind::PartsList(PartsList {
        of: SheetComponent { source, component: 0 },
        view: View::default(),
        group: Group::default(),
        fold: false,
        purchased_contents: false,
        mass: MassFormat::default(),
    });
    let first = d.allocate_id(&mut id_source).unwrap();
    let second = d.allocate_id(&mut id_source).unwrap();
    for (id, kind) in [(first, list), (second, Kind::Revision)] {
        d.tables.add([Arc::new(SheetTable { id, sheet, table: kind.default_table(), place: kind.default_place(), kind })]);
    }
    d.validate().unwrap();
    (d, first)
}

/// `change` on the saved drawing is refused on reading, for the reason given.
fn damaged(d: &Drawing, change: impl Fn(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
    change(&mut v);
    Drawing::from_value(v).unwrap_err().to_string()
}

/// A table saves as it is, in any order read back by id, and what a table doesn't set stays out.
#[test]
fn tables_save_and_read_back_by_id() {
    let (d, first) = tabled();
    let saved = d.to_json().unwrap();
    assert_eq!(Drawing::from_json(&saved).unwrap(), d);
    let mut v: Value = serde_json::from_str(&saved).unwrap();
    v["tables"].as_array_mut().unwrap().reverse();
    assert_eq!(Drawing::from_value(v).unwrap(), d, "in id order whatever the file's order");
    assert!(d.tables.by_id(first).is_some() && d.tables.by_id(1).is_none());
    let list = serde_json::to_value(d.tables.by_id(first).unwrap()).unwrap();
    assert_eq!(list["kind"]["parts_list"]["levels"], json!("flat"));
    assert_eq!(list["kind"]["parts_list"].get("group_children"), None);
    let no_tables: Value = serde_json::from_str(&Drawing { tables: Default::default(), ..d.clone() }.to_json().unwrap()).unwrap();
    assert_eq!(no_tables.get("tables"), None, "a drawing with none saves as it did");
}

/// A parts list's request says what the list lists, unweighed; defaults are what the plan says.
#[test]
fn a_list_asks_for_its_view() {
    let list = PartsList {
        of: SheetComponent { source: 5, component: 9 },
        view: View::AllLevels,
        group: Group::Definition,
        fold: true,
        purchased_contents: true,
        mass: MassFormat::default(),
    };
    let request = list.request();
    assert_eq!(
        (request.source, request.view, request.group, request.fold, request.purchased_contents),
        (9, View::AllLevels, Group::Definition, true, true)
    );
    assert!(!request.mass);
    let names = |kind: &Kind| kind.default_table().columns.into_iter().map(|c| c.name).collect::<Vec<_>>();
    assert_eq!(names(&Kind::PartsList(list)), ["item", "quantity", "name", "description", "material", "mass", "ballooned"]);
    assert_eq!(names(&Kind::Revision), ["revision", "description", "designer", "checker", "date"]);
    // How many balloons point at each row, for the editor alone: the only column not printed.
    let unprinted = |kind: &Kind| kind.default_table().columns.iter().filter(|c| !c.printed).map(|c| c.heading.clone()).collect::<Vec<_>>();
    assert_eq!((unprinted(&Kind::PartsList(list)), unprinted(&Kind::Revision)), (vec!["BALLOONED".to_string()], vec![]));
    assert_eq!(Kind::Revision.default_place(), Place::Anchor { corner: Corner::BottomLeft, offset_mm: [0.0, 0.0] });
    for kind in [Kind::PartsList(list), Kind::Revision] {
        kind.default_table().validate().unwrap();
    }
}

/// A table's id can't be another object's, and a sheet counts and gives up its tables only.
#[test]
fn a_table_shares_the_drawings_ids_and_goes_with_its_sheet() {
    let (d, first) = tabled();
    let mut seen = BTreeSet::new();
    assert!(d.ids().all(|id| seen.insert(id)), "each once");
    assert!(seen.contains(&first));
    let sheet = d.sheets[0].id;
    let why = damaged(&d, |v| v["tables"][0]["id"] = v["sheets"][0]["id"].clone());
    assert!(why.contains("id"), "{why}");

    let on = d.on_sheet(sheet);
    assert_eq!(on.tables.len(), 2);
    assert!(!on.is_empty());
    let mut one = d.clone();
    one.remove_objects(&OnSheet { tables: vec![first], ..Default::default() });
    assert_eq!(one.tables.len(), 1);
    assert!(one.tables.by_id(first).is_none());
    one.validate().unwrap();
}

/// A damaged table is refused, each rule by its own words.
#[test]
fn a_damaged_table_is_refused() {
    let (d, _) = tabled();
    let at = |why: &str, change: &dyn Fn(&mut Value)| {
        let got = damaged(&d, change);
        assert!(got.contains(why), "{why}: {got}");
    };
    at("sheet", &|v| v["tables"][0]["sheet"] = json!(crate::ids::encode(99)));
    at("source", &|v| v["tables"][0]["kind"]["parts_list"]["of"]["source"] = json!(crate::ids::encode(99)));
    at("decimals", &|v| v["tables"][0]["kind"]["parts_list"]["mass"]["decimals"] = json!(7));
    at("row's height", &|v| v["tables"][0]["table"]["row_height_mm"] = json!(0.0));
    at("columns", &|v| v["tables"][0]["table"]["columns"] = json!([]));
    at("on paper", &|v| v["tables"][1]["place"] = json!({"at": [6000.0, 0.0]}));
    at("on paper", &|v| v["tables"][1]["place"]["anchor"]["offset_mm"] = json!([0.0, -6000.0]));
    assert!(serde_json::from_value::<SheetTable>(json!({"id": "x"})).is_err());
    let unknown = damaged(&d, |v| v["tables"][0]["extra"] = json!(1));
    assert!(unknown.contains("extra"), "{unknown}");
}

/// More tables than a drawing holds are refused, and by id when out of order.
#[test]
fn the_tables_cap_holds() {
    let (mut d, first) = tabled();
    let one = d.tables.by_id(first).unwrap().clone();
    *d.tables = (0..=MAX_TABLES as u64).map(|i| Arc::new(SheetTable { id: 1 << 45 | i, ..one.clone() })).collect();
    assert!(d.validate().unwrap_err().to_string().contains("at most 1000 tables"));
    d.tables.truncate(MAX_TABLES);
    d.validate().unwrap();
    d.tables.reverse();
    assert!(d.validate().unwrap_err().to_string().contains("by id"));
}
