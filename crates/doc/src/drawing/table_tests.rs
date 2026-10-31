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

fn parts_list(mass: MassFormat) -> PartsList {
    PartsList {
        of: SheetComponent { source: 1, component: 0 },
        view: View::default(),
        group: Group::default(),
        fold: false,
        purchased_contents: false,
        mass,
    }
}

/// A row listing `count` of component `id`, numbered `item`.
fn bom_row(id: u64, item: u32, count: u64) -> bom::Row {
    bom::Row {
        key: bom::RowKey { parents: Vec::new(), member: bom::Member { component: id, bom_type: BomType::Normal } },
        item,
        provisional: false,
        locked: false,
        duplicate: false,
        bom_type: BomType::Normal,
        assembly: false,
        stands_for: bom::Holds::All,
        components: vec![id],
        names: Vec::new(),
        part_number: String::new(),
        description: String::new(),
        revision: String::new(),
        properties: Default::default(),
        materials: Vec::new(),
        statics: Vec::new(),
        count,
        each: count,
        quantity: Quantity::Count,
        members: Vec::new(),
        mass: Mass::NotAsked,
        children: Vec::new(),
    }
}

fn bom_of(rows: Vec<bom::Row>) -> Bom {
    let mut list = Bom::default();
    list.rows = rows;
    list
}

/// Every value of a row reads as a list shows it, a value it hasn't got is left out for the table to
/// mark, and a free property takes no value's place.
#[test]
fn a_rows_values_read_as_a_list_shows_them() {
    let mut row = bom_row(7, 3, 4);
    row.names = vec!["Bolt".into(), "Bolt B".into()];
    row.part_number = "B-200".into();
    row.description = "M8".into();
    row.revision = "C".into();
    row.materials = vec!["Steel".into(), "Zinc".into()];
    row.bom_type = BomType::Purchased;
    row.mass = Mass::Grams(1250.0);
    row.properties.insert("finish".into(), "Black".into());
    row.properties.insert("item".into(), "not the item".into());
    let list = bom_of(vec![row]);
    let rows = parts_list(MassFormat::default()).rows(&list, "mm");
    let fields = &rows[0].fields;
    let read = |key: &str| fields.get(key).map(String::as_str);
    assert_eq!((read("item"), read("quantity"), read("count"), read("name")), (Some("3"), Some("4"), Some("4"), Some("Bolt; Bolt B")));
    assert_eq!((read("part_number"), read("description"), read("revision")), (Some("B-200"), Some("M8"), Some("C")));
    assert_eq!((read("material"), read("mass"), read("finish")), (Some("Steel; Zinc"), Some("1.25"), Some("Black")));
    assert!(rows[0].purchased);
    assert_eq!(rows[0].kind, table::RowKind::Part);

    for (mass, format, shown) in [
        (Mass::Grams(1250.0), MassFormat { unit: MassUnit::G, decimals: 0 }, Some("1250")),
        (Mass::Grams(1250.0), MassFormat { unit: MassUnit::Kg, decimals: 3 }, Some("1.250")),
        (Mass::Grams(453.59237), MassFormat { unit: MassUnit::Lb, decimals: 2 }, Some("1.00")),
        (Mass::Grams(f64::NAN), MassFormat::default(), None),
        (Mass::Grams(f64::INFINITY), MassFormat::default(), None),
        (Mass::NotAsked, MassFormat::default(), None),
        (Mass::Unknown(bom::Unknown::Differs), MassFormat::default(), None),
    ] {
        let mut row = bom_row(7, 1, 1);
        row.mass = mass;
        let rows = parts_list(format).rows(&bom_of(vec![row]), "mm");
        assert_eq!(rows[0].fields.get("mass").map(String::as_str), shown, "{mass:?}");
    }
    for (quantity, units, shown) in [
        (Quantity::Count, "mm", Some("1")),
        (Quantity::Pieces(6.0), "mm", Some("6")),
        (Quantity::Length(1250.0), "mm", Some("1250.00 mm")),
        (Quantity::Length(1250.0), "m", Some("1.25 m")),
        (Quantity::Unknown(bom::Uncounted::Mixed), "mm", None),
    ] {
        let mut row = bom_row(7, 1, 1);
        row.quantity = quantity;
        let rows = parts_list(MassFormat::default()).rows(&bom_of(vec![row]), units);
        assert_eq!(rows[0].fields.get("quantity").map(String::as_str), shown, "{quantity:?} {units}");
    }
}

/// A row listed under another reads its full item and how many are in one of its parent, and keys
/// apart from the same part listed elsewhere. Worked by hand on a fixed-width measure of half the cap
/// height a letter: the table's outline and each cell's corners, with a value it hasn't got marked.
#[test]
fn a_lists_rows_lay_out_in_a_table() {
    let mut inner = bom_row(5, 1, 6);
    inner.each = 2;
    let mut outer = bom_row(4, 2, 3);
    outer.assembly = true;
    inner.key.parents = vec![outer.key.member];
    outer.children = vec![inner];
    outer.names = vec!["Frame".into()];
    let mut other = bom_row(5, 3, 1);
    other.names = vec!["Peg".into()];
    let list = bom_of(vec![outer, other]);
    let parts = parts_list(MassFormat::default());
    let rows = parts.rows(&list, "mm");
    let read = |row: usize, key: &str| rows[row].fields.get(key).cloned();
    assert_eq!(rows.iter().map(|r| r.fields["item"].as_str()).collect::<Vec<_>>(), ["2", "2.1", "3"]);
    assert_eq!((read(1, "quantity"), read(1, "count")), (Some("2".into()), Some("6".into())));
    assert_eq!(rows.iter().map(|r| r.kind).collect::<Vec<_>>(), [table::RowKind::Assembly, table::RowKind::Part, table::RowKind::Part]);
    assert_ne!(rows[1].key, rows[2].key, "one part listed in two places");

    let kind = Kind::PartsList(parts);
    let definition = kind.default_table();
    let measure = |text: &str, cap: f64| text.chars().count() as f64 * cap / 2.0;
    let laid = definition.layout(&rows, solidcore_record::Vec2::new(400.0, 280.0), &measure).unwrap();
    assert_eq!(laid.rows.len(), 3);
    // ITEM, QTY, FRAME's NAME, DESCRIPTION, MATERIAL, MASS: letters at 1.75 mm, a 1 mm pad each side;
    // the outer two also hold half an outline line and half a cell line, the rest two half cell lines.
    let letters = [4.0, 3.0, 5.0, 11.0, 8.0, 4.0];
    let width: f64 = letters.iter().map(|n: &f64| n * 1.75 + 2.0).sum::<f64>() + 2.0 * 0.35 + 5.0 * 0.35;
    assert!((laid.outline.max.x - 400.0).abs() < 1e-9 && (laid.outline.min.x - (400.0 - width)).abs() < 1e-9, "{:?} {width}", laid.outline);
    // Four rows of 7 mm down from the corner.
    assert!((laid.outline.min.y - (280.0 - 4.0 * 7.0)).abs() < 1e-9, "{:?}", laid.outline);
    let row = &laid.rows[1];
    assert!((row.rect.max.y - (280.0 - 2.0 * 7.0)).abs() < 1e-9 && (row.rect.min.y - (280.0 - 3.0 * 7.0)).abs() < 1e-9);
    assert_eq!(row.cells[0].lines[0].text, "2.1");
    // Not weighed: its cell shows the missing value (`cell::MISSING`, nothing), so it has no line.
    assert!(row.cells[5].lines.is_empty(), "not weighed");
}

/// A row's key fits at the deepest a list goes: 64 levels under the top, every member its longest,
/// each key apart.
#[test]
fn a_row_at_the_deepest_level_keeps_its_key() {
    let mut row = bom_row(u64::MAX, 1, 1);
    row.bom_type = BomType::Inseparable;
    row.key.member.bom_type = BomType::Inseparable;
    for level in 0..64u64 {
        let mut parent = bom_row(u64::MAX - level - 1, 1, 1);
        parent.key.member.bom_type = BomType::Inseparable;
        let member = parent.key.member;
        fn under(row: &mut bom::Row, member: bom::Member) {
            row.key.parents.insert(0, member);
            row.children.iter_mut().for_each(|child| under(child, member));
        }
        under(&mut row, member);
        parent.children = vec![row];
        row = parent;
    }
    let list = bom_of(vec![row]);
    let parts = parts_list(MassFormat::default());
    let rows = parts.rows(&list, "mm");
    assert_eq!(rows.len(), 65);
    let deepest = &rows[64];
    assert_eq!(deepest.key.split('/').count(), 65);
    assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<BTreeSet<_>>().len(), 65);
    let definition = Kind::PartsList(parts).default_table();
    definition.layout(&rows, solidcore_record::Vec2::new(400.0, 280.0), &|t, cap| t.len() as f64 * cap / 2.0).unwrap();
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
