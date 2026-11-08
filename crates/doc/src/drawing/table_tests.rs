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

/// Each row says how deep it is listed and gives its numbers as numbers: its count, its quantity (a
/// length in millimetres whatever the drawing's units) and its mass in grams, none it hasn't got.
/// Sorted by name, descending, a row listed under another stays under it.
#[test]
fn a_lists_rows_carry_their_level_and_numbers_and_sort_within_each_level() {
    let mut inner = bom_row(5, 1, 6);
    inner.each = 2;
    inner.names = vec!["Washer".into()];
    inner.mass = Mass::Grams(12.5);
    let mut outer = bom_row(4, 2, 3);
    outer.assembly = true;
    outer.names = vec!["Frame".into()];
    inner.key.parents = vec![outer.key.member];
    outer.children = vec![inner];
    let mut other = bom_row(6, 3, 1);
    other.names = vec!["Peg".into()];
    other.quantity = Quantity::Length(1250.0);
    let parts = parts_list(MassFormat::default());
    let rows = parts.rows(&bom_of(vec![outer, other]), "m");
    assert_eq!(rows.iter().map(|r| r.level).collect::<Vec<_>>(), [0, 1, 0]);
    let numbers = |row: usize| rows[row].numbers.iter().map(|(k, n)| (k.as_str(), *n)).collect::<Vec<_>>();
    assert_eq!(numbers(0), [("count", 3.0), ("quantity", 3.0)]);
    assert_eq!(numbers(1), [("count", 6.0), ("mass", 12.5), ("quantity", 2.0)]);
    assert_eq!(numbers(2), [("count", 1.0), ("quantity", 1250.0)], "a length in millimetres, shown in metres");
    let mut definition = Kind::PartsList(parts).default_table();
    definition.sort = vec![table::SortKey { column: "name".into(), descending: true }];
    let laid = definition.layout(&rows, solidcore_record::Vec2::new(400.0, 280.0), &|t, cap| t.len() as f64 * cap / 2.0).unwrap();
    let items: Vec<&str> = laid.rows.iter().map(|r| r.cells[0].lines[0].text.as_str()).collect();
    assert_eq!(items, ["3", "2", "2.1"], "Peg, then Frame with its Washer under it");
}

/// A sorted table saves its sort and reads back as it was; one that isn't saves as before.
#[test]
fn a_tables_sort_saves_and_reads_back() {
    let (mut d, first) = tabled();
    let at = d.tables.index_of(first).unwrap();
    Arc::make_mut(&mut d.tables[at]).table.sort =
        vec![table::SortKey { column: "mass".into(), descending: true }, table::SortKey { column: "item".into(), descending: false }];
    let saved: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
    let tables = saved["tables"].as_array().unwrap();
    assert_eq!(tables[0]["table"]["sort"], json!([{"column": "mass", "descending": true}, {"column": "item"}]));
    assert_eq!(tables[1]["table"].get("sort"), None, "the revision table isn't sorted");
    assert_eq!(Drawing::from_value(saved).unwrap(), d);
    let wrong = damaged(&d, |v| v["tables"][0]["table"]["sort"][0]["column"] = json!("colour"));
    assert!(wrong.contains("a sort key names a column"), "{wrong}");
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

/// A design with a width and a hole parameter, a body's material and a feature's suppression in its
/// configuration table, and four configurations, Large active.
fn configured() -> Document {
    use crate::config::{Column as ConfigColumn, ConfigRow};
    let mut doc = Document::new("Bracket");
    doc.configs.columns = vec![
        ConfigColumn::Param("width".into()),
        ConfigColumn::Param("hole_d".into()),
        ConfigColumn::Material("Body 1".into()),
        ConfigColumn::Suppress(99),
    ];
    let row =
        |name: &str, width: &str, hole: &str| ConfigRow { name: name.into(), cells: vec![width.into(), hole.into(), "Steel".into(), "false".into()] };
    doc.configs.rows =
        vec![row("Default", "20 mm", "5 mm"), row("Small", "10 mm", "3 mm"), row("Large", "9 mm", "8 mm"), row("Medium", "w * 2", "6 mm")];
    doc.configs.active = Some("Large".into());
    doc
}

/// A family table lists each configuration in its table's order, by its name, with each of its
/// cells as the design saves it and the active one marked; every row on the first level, with no
/// numbers, so its cells sort as text.
#[test]
fn a_family_table_lists_each_configuration_in_order() {
    let doc = configured();
    let rows = Family::rows(&doc);
    assert_eq!(rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["Default", "Small", "Large", "Medium"]);
    let read = |row: usize, key: &str| rows[row].fields.get(key).map(String::as_str);
    assert_eq!((read(0, "name"), read(0, "param:width"), read(0, "param:hole_d")), (Some("Default"), Some("20 mm"), Some("5 mm")));
    assert_eq!((read(3, "param:width"), read(3, "material:Body 1"), read(3, "suppress:99")), (Some("w * 2"), Some("Steel"), Some("false")));
    assert_eq!(rows.iter().map(|r| r.fields.get(ACTIVE).map(String::as_str)).collect::<Vec<_>>(), [None, None, Some("yes"), None]);
    assert!(rows.iter().all(|r| r.level == 0 && r.numbers.is_empty() && r.kind == table::RowKind::Part && !r.purchased));
    assert!(Family::rows(&Document::new("None")).is_empty(), "no configurations, no rows");

    let family = Family { of: SheetComponent { source: 1, component: 0 } };
    let (mut definition, left_out) = family.table(&doc);
    assert_eq!(left_out, LeftOut::default());
    definition.sort = vec![table::SortKey { column: "param_width".into(), descending: false }];
    let measure = |text: &str, cap: f64| text.chars().count() as f64 * cap / 2.0;
    let laid = definition.layout(&rows, solidcore_record::Vec2::new(0.0, 280.0), &measure).unwrap();
    let cell = |row: usize, at: usize| laid.rows[row].cells[at].lines.first().map(|l| l.text.clone());
    // Natural text order: 9 mm before 10 mm, and an expression after the numbers' text.
    assert_eq!((0..4).map(|row| cell(row, 1)).collect::<Vec<_>>(), ["9 mm", "10 mm", "20 mm", "w * 2"].map(|t| Some(t.to_string())));
    assert_eq!(cell(0, 0).as_deref(), Some("Large"));

    // A configuration column the design no longer has shows the missing value; the rest still read.
    let mut fewer = doc.clone();
    fewer.configs.columns.remove(1);
    for row in &mut fewer.configs.rows {
        row.cells.remove(1);
    }
    let laid = definition.layout(&Family::rows(&fewer), solidcore_record::Vec2::new(0.0, 280.0), &measure).unwrap();
    assert!(laid.rows.iter().all(|row| row.cells[2].lines.is_empty()), "HOLE_D is gone from the design");
    assert_eq!(laid.rows[0].cells[1].lines[0].text, "9 mm");
}

/// A family table's columns: NAME, then one per configuration column, headed by the parameter's
/// name or the column's kind and target in capitals, named by its key in snake case, numbered on a
/// clash. Those a table has no room for, or that no row could fill or no label show, are counted;
/// appearance columns are left out and counted on their own, as a look can't be printed as text.
#[test]
fn a_family_tables_columns_have_headings_names_and_a_cap() {
    use crate::config::Column as ConfigColumn;
    let family = Family { of: SheetComponent { source: 1, component: 0 } };
    let (definition, left_out) = family.table(&configured());
    let read = |f: fn(&Column) -> String| definition.columns.iter().map(f).collect::<Vec<_>>();
    assert_eq!(read(|c| c.heading.clone()), ["NAME", "WIDTH", "HOLE_D", "MATERIAL BODY 1", "SUPPRESS 99"]);
    assert_eq!(read(|c| c.name.clone()), ["name", "param_width", "param_hole_d", "material_body_1", "suppress_99"]);
    assert_eq!(left_out, LeftOut::default());
    assert!(definition.columns.iter().all(|c| c.printed && c.align == HAlign::Left));
    definition.validate().unwrap();

    let mut doc = Document::new("Clashes");
    doc.configs.columns = ["a b", "a_b", "a-b", "Ä"].map(|n| ConfigColumn::Param(n.into())).into();
    let (definition, _) = family.table(&doc);
    assert_eq!(definition.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["name", "param_a_b", "param_a_b_2", "param_a_b_3", "param"]);
    definition.validate().unwrap();

    for (count, kept, out) in [(62, 63, 0), (63, 64, 0), (64, 64, 1), (500, 64, 437)] {
        doc.configs.columns = (0..count).map(|i| ConfigColumn::Param(format!("p{i}"))).collect();
        let (definition, left_out) = family.table(&doc);
        assert_eq!((definition.columns.len(), left_out), (kept, LeftOut { looks: 0, others: out }), "{count} configuration columns");
        definition.validate().unwrap();
    }
    // A key no row field can carry, and a heading on two lines, are left out and counted.
    let long = "x".repeat(table::MAX_KEY_BYTES);
    doc.configs.columns = [long.as_str(), "line\nbreak", "kept"].map(|n| ConfigColumn::Param(n.into())).into();
    let (definition, left_out) = family.table(&doc);
    assert_eq!((definition.columns.len(), left_out), (2, LeftOut { looks: 0, others: 2 }));
    assert_eq!(definition.columns[1].name, "param_kept");
    let long_name = format!("param_{}", "y".repeat(80));
    doc.configs.columns = vec![ConfigColumn::Param(long_name.clone()), ConfigColumn::Param(long_name)];
    let (definition, _) = family.table(&doc);
    definition.validate().unwrap();
    assert!(definition.columns.iter().all(|c| c.name.len() <= MAX_NAME_BYTES), "short enough to number");

    // Looks stay out whatever room is left, and count apart from the rest.
    let mut looks: Vec<ConfigColumn> = (0..64).map(|i| ConfigColumn::Param(format!("p{i}"))).collect();
    looks.splice(1..1, [ConfigColumn::Appearance("Body 1".into()), ConfigColumn::ComponentAppearance(3), ConfigColumn::Material("Body 1".into())]);
    doc.configs.columns = looks;
    let (definition, left_out) = family.table(&doc);
    assert_eq!(left_out, LeftOut { looks: 2, others: 2 }, "62 parameters and the material fit beside NAME");
    assert_eq!(definition.columns.iter().take(4).map(|c| c.name.as_str()).collect::<Vec<_>>(), ["name", "param_p0", "material_body_1", "param_p1"]);
    assert!(definition.columns.iter().all(|c| !c.name.contains("appearance")));
}

/// A family table saves and reads back, starts at the top left, points no balloon at it, and is
/// refused when it names a source the drawing lacks.
#[test]
fn a_family_table_saves_and_its_source_is_checked() {
    let (mut d, _) = tabled();
    let mut id_source = IdSource::seeded(13056);
    let id = d.allocate_id(&mut id_source).unwrap();
    let kind = Kind::Family(Family { of: SheetComponent { source: d.sources[0].id, component: 0 } });
    assert_eq!(kind.default_place(), Place::Anchor { corner: Corner::TopLeft, offset_mm: [0.0, 0.0] });
    let (table, _) = Family { of: SheetComponent { source: d.sources[0].id, component: 0 } }.table(&configured());
    let sheet = d.sheets[0].id;
    d.tables.add([Arc::new(SheetTable { id, sheet, table, place: kind.default_place(), kind })]);
    d.validate().unwrap();
    let saved = d.to_json().unwrap();
    assert_eq!(Drawing::from_json(&saved).unwrap(), d);
    let v: Value = serde_json::from_str(&saved).unwrap();
    let family = v["tables"].as_array().unwrap().iter().find(|t| t["kind"].get("family").is_some()).cloned().unwrap();
    assert_eq!(family["kind"]["family"]["of"]["component"], json!(0), "{family}");
    let why = damaged(&d, |v| {
        for t in v["tables"].as_array_mut().unwrap() {
            if let Some(of) = t["kind"].get_mut("family") {
                of["of"]["source"] = json!(crate::ids::encode(99));
            }
        }
    });
    assert!(why.contains("source"), "{why}");
    let unknown = damaged(&d, |v| {
        for t in v["tables"].as_array_mut().unwrap() {
            if let Some(of) = t["kind"].get_mut("family") {
                of["columns"] = json!([]);
            }
        }
    });
    assert!(unknown.contains("columns"), "{unknown}");
}
