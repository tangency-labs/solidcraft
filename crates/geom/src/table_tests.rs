use crate::prelude::*;
use serde_json::json;

use super::*;
use crate::cell::Format;

/// Every character 0.6 cap heights wide: 1.5 mm at the tests' 2.5 mm text.
fn fixed(text: &str, cap_mm: f64) -> f64 {
    text.chars().count() as f64 * 0.6 * cap_mm
}

fn field(key: &str) -> Content {
    Content(vec![Part::Field { source: Source::Row, key: key.into(), format: Format::default() }])
}

fn column(name: &str, heading: &str, align: HAlign) -> Column {
    Column { name: name.into(), heading: heading.into(), content: field(name), width_mm: None, align, printed: true }
}

/// Item, part number and quantity: 2.5 mm text, a 0.7 mm outline and 0.35 mm lines between cells.
fn table() -> Table {
    Table {
        columns: vec![
            column("item", "Item", HAlign::Centre),
            column("part_number", "Part number", HAlign::Left),
            column("quantity", "Qty", HAlign::Right),
        ],
        heading: Heading::Top,
        row_height_mm: 7.0,
        cap_mm: 2.5,
        outline_line_mm: 0.7,
        cell_line_mm: 0.35,
        filter: Filter::default(),
        overrides: vec![],
        split: None,
        sort: Vec::new(),
    }
}

fn row(key: &str, kind: RowKind, purchased: bool, fields: &[(&str, &str)]) -> Row {
    Row {
        key: key.into(),
        kind,
        purchased,
        fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        level: 0,
        numbers: BTreeMap::new(),
    }
}

/// A plate, a bought pin and a frame.
fn rows() -> Vec<Row> {
    vec![
        row("plate", RowKind::Part, false, &[("item", "1"), ("part_number", "PL-100"), ("quantity", "3")]),
        row("pin", RowKind::Part, true, &[("item", "2"), ("part_number", "PN-040"), ("quantity", "2")]),
        row("frame", RowKind::Assembly, false, &[("item", "3"), ("part_number", "FR-1"), ("quantity", "1")]),
    ]
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Aabb2 {
    Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) }
}

fn near(a: Aabb2, b: Aabb2) -> bool {
    a.min.dist(b.min) < 1e-9 && a.max.dist(b.max) < 1e-9
}

fn at(a: Vec2, x: f64, y: f64) -> bool {
    a.dist(Vec2::new(x, y)) < 1e-9
}

fn texts(cell: &CellLayout) -> Vec<&str> {
    cell.lines.iter().map(|l| l.text.as_str()).collect()
}

/// By hand: a fitted column is its widest line + 2 mm of padding + half of each line beside it
/// (0.35 on the outline, 0.175 between cells). Item: "Item", 6 mm, so 8.525. Part number: "Part
/// number", 16.5 mm, so 18.85. Qty: "Qty", 4.5 mm, so 7.025. 34.4 mm in all. One line needs 2.5 +
/// 2 + the halves above and below, at most 5.2 mm, so every row is 7 mm.
const WIDTHS: [f64; 3] = [8.525, 18.85, 7.025];

#[test]
fn fitted_columns_with_the_heading_at_the_top() {
    let layout = table().layout(&rows(), Vec2::new(400.0, 280.0), &fixed).unwrap();
    assert!(near(layout.outline, rect(365.6, 252.0, 400.0, 280.0)), "{:?}", layout.outline);
    assert!(near(layout.heading.rect, rect(365.6, 273.0, 400.0, 280.0)));
    let keys: Vec<_> = layout.rows.iter().map(|r| r.key.as_deref().unwrap()).collect();
    assert_eq!(keys, ["plate", "pin", "frame"]);
    for (k, (r, y0)) in layout.rows.iter().zip([266.0, 259.0, 252.0]).enumerate() {
        assert!(near(r.rect, rect(365.6, y0, 400.0, y0 + 7.0)), "row {k}: {:?}", r.rect);
        let mut x = 365.6;
        for (cell, w) in r.cells.iter().zip(WIDTHS) {
            assert!(near(cell.rect, rect(x, y0, x + w, y0 + 7.0)), "row {k}: {:?}", cell.rect);
            x += w;
        }
    }
    // The plate's item, centred: inside its lines 365.95..373.95 by 266.175..272.825, its value box
    // 1 mm in, one line centred on it: x 369.95, baseline 269.5 - 1.25.
    let item = &layout.rows[0].cells[0];
    assert_eq!(texts(item), ["1"]);
    assert!(at(item.lines[0].at, 369.95, 268.25), "{:?}", item.lines[0].at);
    // The frame's quantity, on the bottom row, right-aligned: its right side 400 - 0.35 - 1, the box
    // 252.35 + 1 .. 259 - 0.175 - 1.
    let qty = &layout.rows[2].cells[2];
    assert!(at(qty.lines[0].at, 398.65, (253.35 + 257.825) / 2.0 - 1.25), "{:?}", qty.lines[0].at);
    // The heading's part number, left: 374.125 + 0.175 + 1, between 273.175 + 1 and 280 - 0.35 - 1.
    assert!(at(layout.heading.cells[1].lines[0].at, 375.3, (274.175 + 278.65) / 2.0 - 1.25));
    assert!(layout.rows.iter().flat_map(|r| &r.cells).all(|c| c.warning.is_none() && c.replaced.is_none()));
}

#[test]
fn with_the_heading_at_the_bottom_item_1_sits_just_above_it() {
    let table = Table { heading: Heading::Bottom, ..table() };
    let layout = table.layout(&rows(), Vec2::new(400.0, 10.0), &fixed).unwrap();
    assert!(near(layout.outline, rect(365.6, 10.0, 400.0, 38.0)));
    assert!(near(layout.heading.rect, rect(365.6, 10.0, 400.0, 17.0)));
    let placed: Vec<_> = layout.rows.iter().map(|r| (r.key.as_deref().unwrap(), r.rect.min.y)).collect();
    assert_eq!(placed, [("plate", 17.0), ("pin", 24.0), ("frame", 31.0)], "listed in order, growing up");
    // The heading's outer edge is now its bottom: its text box 10.35 + 1 .. 17 - 0.175 - 1.
    assert!(at(layout.heading.cells[1].lines[0].at, 375.3, (11.35 + 15.825) / 2.0 - 1.25));
}

#[test]
fn a_set_width_wraps_between_words_and_grows_the_row() {
    let mut table = table();
    table.columns.insert(2, Column { width_mm: Some(20.0), ..column("description", "Description", HAlign::Left) });
    let mut rows = rows();
    rows[0].fields.insert("description".into(), "Plate, 3 mm   thick steel".into());
    rows[1].fields.insert("description".into(), "Supercalifragilistic pin".into());
    rows[2].fields.insert("description".into(), "Frame\nwelded".into());
    let layout = table.layout(&rows, Vec2::new(400.0, 280.0), &fixed).unwrap();
    // 20 mm less the two 0.175 halves and 2 mm of padding leaves 17.65: eleven characters a line.
    let plate = &layout.rows[0].cells[2];
    assert_eq!(texts(plate), ["Plate, 3 mm", "thick steel"], "broken between words, a run of spaces kept as one");
    // Two lines: 2.5 + 2.5 × 10/7 + 2 + 0.35 = 8.421… mm, more than 7.
    let pitch = 2.5 * LINE_PITCH;
    let height = 2.5 + pitch + 2.0 + 0.35;
    let r = &layout.rows[0].rect;
    assert!((r.size().y - height).abs() < 1e-9, "{r:?}");
    assert!((layout.rows[1].rect.max.y - r.min.y).abs() < 1e-9, "the next row starts under it");
    // From the top of the box: the first baseline a cap height under it, the next a pitch lower.
    let top = r.max.y - 0.175 - 1.0;
    assert!((plate.lines[0].at.y - (top - 2.5)).abs() < 1e-9 && (plate.lines[1].at.y - (top - 2.5 - pitch)).abs() < 1e-9);
    assert!((plate.lines[0].at.x - plate.lines[1].at.x).abs() < 1e-9);
    // A word wider than the column is shown whole, with a warning.
    let pin = &layout.rows[1].cells[2];
    assert_eq!(texts(pin), ["Supercalifragilistic", "pin"]);
    assert_eq!(pin.warning, Some(Warning::Width));
    // Line breaks are kept; both lines fit.
    assert_eq!(texts(&layout.rows[2].cells[2]), ["Frame", "welded"]);
    assert_eq!(layout.rows[2].cells[2].warning, None);
    // A fitted column grows instead: the description as one line.
    table.columns[2].width_mm = None;
    let fitted = table.layout(&rows, Vec2::new(400.0, 280.0), &fixed).unwrap();
    assert_eq!(texts(&fitted.rows[0].cells[2]), ["Plate, 3 mm   thick steel"]);
    assert!((fitted.rows[0].cells[2].rect.size().x - (25.0 * 1.5 + 2.35)).abs() < 1e-9);
}

#[test]
fn overrides_show_their_text_and_keep_the_value() {
    let mut table = table();
    table.overrides = vec![
        Override { row: "plate".into(), column: "part_number".into(), text: "PL-100-A".into() },
        Override { row: "pin".into(), column: "quantity".into(), text: String::new() },
        Override { row: "gone".into(), column: "quantity".into(), text: "9".into() },
    ];
    let mut rows = rows();
    rows[2].fields.remove("quantity");
    let layout = table.layout(&rows, Vec2::new(400.0, 280.0), &fixed).unwrap();
    let plate = &layout.rows[0].cells[1];
    assert_eq!((texts(plate), plate.replaced.as_deref()), (vec!["PL-100-A"], Some("PL-100")));
    let pin = &layout.rows[1].cells[2];
    assert_eq!((texts(pin), pin.replaced.as_deref()), (vec![], Some("2")), "a blank cell, still tracked");
    let frame = &layout.rows[2].cells[2];
    assert_eq!((texts(frame), frame.replaced.as_deref()), (vec![], None), "a field the row doesn't have shows nothing");
    assert_eq!(layout.rows.len(), 3, "an override of a row that has gone is kept and shows nowhere");
    // The fitted part number column is as wide as the override: "Part number" is still wider.
    assert!((layout.rows[0].cells[1].rect.size().x - WIDTHS[1]).abs() < 1e-9);
    // Text around a field: the field's value inside it.
    table.columns[0].content.0.insert(0, Part::Text("#".into()));
    let layout = table.layout(&rows, Vec2::new(400.0, 280.0), &fixed).unwrap();
    assert_eq!(texts(&layout.rows[1].cells[0]), ["#2"]);
}

#[test]
fn filters_hide_rows_and_never_change_a_value() {
    let shown = |filter: Filter| {
        let table = Table { filter, ..table() };
        let layout = table.layout(&rows(), Vec2::new(400.0, 280.0), &fixed).unwrap();
        layout.rows.iter().map(|r| (r.key.clone().unwrap(), texts(&r.cells[0]).concat(), texts(&r.cells[2]).concat())).collect::<Vec<_>>()
    };
    let all = shown(Filter::default());
    let only = |only: Only| shown(Filter { only: Some(only), text: vec![] });
    let keys = |rows: Vec<(String, String, String)>| rows.into_iter().map(|r| r.0).collect::<Vec<_>>();
    assert_eq!(keys(only(Only::Parts)), ["plate", "pin"]);
    assert_eq!(keys(only(Only::Assemblies)), ["frame"]);
    assert_eq!(keys(only(Only::Purchased)), ["pin"]);
    let text = |column: &str, contains: &str| Filter { only: None, text: vec![TextFilter { column: column.into(), contains: contains.into() }] };
    assert_eq!(keys(shown(text("part_number", "pl-"))), ["plate"], "ignoring case");
    assert_eq!(keys(shown(text("part_number", "-"))), ["plate", "pin", "frame"]);
    assert!(shown(text("part_number", "nothing")).is_empty());
    let both = Filter { only: Some(Only::Parts), text: vec![TextFilter { column: "quantity".into(), contains: "2".into() }] };
    assert_eq!(keys(shown(both)), ["pin"]);
    // Every row shown keeps its item and quantity as the rows give them.
    for filtered in [only(Only::Parts), only(Only::Assemblies), only(Only::Purchased), shown(text("part_number", "pn"))] {
        assert!(filtered.iter().all(|r| all.contains(r)), "{filtered:?}");
    }
    // A filter on what an override shows.
    let mut table = Table { filter: text("part_number", "special"), ..table() };
    table.overrides.push(Override { row: "frame".into(), column: "part_number".into(), text: "Special".into() });
    let layout = table.layout(&rows(), Vec2::new(400.0, 280.0), &fixed).unwrap();
    assert_eq!(layout.rows.iter().map(|r| r.key.clone().unwrap()).collect::<Vec<_>>(), ["frame"]);
}

#[test]
fn a_table_round_trips_through_json_and_unknown_fields_are_refused() {
    let mut table = table();
    table.overrides.push(Override { row: "plate".into(), column: "item".into(), text: "1A".into() });
    table.filter.only = Some(Only::Purchased);
    let json = serde_json::to_value(&table).unwrap();
    assert_eq!(json["columns"][0]["content"], json!([{"field": {"source": "row", "key": "item", "format": {"unit": null, "decimals": null}}}]));
    assert_eq!(serde_json::from_value::<Table>(json.clone()).unwrap(), table);
    // Defaults: the heading at the top, 7 mm rows, no filter, no overrides.
    let least = json!({"columns": [{"name": "item", "heading": "Item", "content": [{"text": "x"}]}], "cap_mm": 2.5,
        "outline_line_mm": 0.7, "cell_line_mm": 0.35});
    let read: Table = serde_json::from_value(least).unwrap();
    assert_eq!((read.heading, read.row_height_mm, &read.filter, read.overrides.len()), (Heading::Top, 7.0, &Filter::default(), 0));
    assert_eq!(read.columns[0].width_mm, None);
    read.validate().unwrap();
    for (path, extra) in [("", "colour"), ("/columns/0", "colour"), ("/filter", "colour"), ("/overrides/0", "colour")] {
        let mut bad = json.clone();
        bad.pointer_mut(path).unwrap().as_object_mut().unwrap().insert(extra.into(), json!(1));
        assert!(serde_json::from_value::<Table>(bad).is_err(), "{path}");
    }
    assert!(serde_json::from_value::<Heading>(json!("left")).is_err());
}

/// A column that isn't printed has no cells, lines or width on paper, and the others keep the places
/// they have without it; its cells are still worked out, so a filter can read them. Saved only when
/// it isn't printed.
#[test]
fn a_column_that_isnt_printed_is_left_off_paper() {
    let mut with = table();
    let note = Column { printed: false, ..column("note", "Note", HAlign::Left) };
    with.columns.insert(1, note.clone());
    with.columns.push(Column { printed: false, ..column(BALLOONED, "Ballooned", HAlign::Centre) });
    let rows: Vec<Row> = rows()
        .into_iter()
        .map(|mut r| {
            r.fields.insert("note".into(), "a long note no column of paper has room for".into());
            r.fields.insert(BALLOONED.into(), "2".into());
            r
        })
        .collect();
    let corner = Vec2::new(400.0, 280.0);
    let (laid, alone) = (with.layout(&rows, corner, &fixed).unwrap(), table().layout(&rows, corner, &fixed).unwrap());
    assert_eq!(laid, alone);
    assert_eq!(laid.paper(9, ObjectKind::PartsList), alone.paper(9, ObjectKind::PartsList));

    with.filter.text.push(TextFilter { column: "note".into(), contains: "NOTE".into() });
    with.overrides.push(Override { row: "pin".into(), column: "note".into(), text: "kept".into() });
    let filtered = with.layout(&rows, corner, &fixed).unwrap();
    assert_eq!(filtered.rows.iter().map(|r| r.key.as_deref().unwrap()).collect::<Vec<_>>(), ["plate", "frame"], "filtered on its text");

    let json = serde_json::to_value(&with).unwrap();
    assert_eq!((json["columns"][0].get("printed"), &json["columns"][1]["printed"]), (None, &json!(false)));
    assert_eq!(serde_json::from_value::<Table>(json).unwrap(), with);
}

/// The ballooned field is for the editor: a printed column that reads it is refused, whatever else it
/// shows. A table prints at least one column.
#[test]
fn the_ballooned_field_is_never_printed() {
    let reads = |content: Vec<Part>, printed: bool| {
        let mut t = table();
        t.columns.push(Column { content: Content(content), printed, ..column("count", "Count", HAlign::Centre) });
        t.validate()
    };
    let ballooned = || Part::Field { source: Source::Row, key: BALLOONED.into(), format: Format::default() };
    let refusal = Err(Invalid("the ballooned column is for the editor only: it isn't printed"));
    assert_eq!(reads(vec![ballooned()], true), refusal);
    assert_eq!(reads(vec![Part::Text("x ".into()), ballooned()], true), refusal);
    assert_eq!(reads(vec![ballooned()], false), Ok(()));
    assert_eq!(reads(vec![Part::Field { source: Source::Row, key: "balloons".into(), format: Format::default() }], true), Ok(()));
    let mut none = table();
    none.columns.iter_mut().for_each(|c| c.printed = false);
    assert_eq!(none.validate(), Err(Invalid("a table prints at least one column")));
}

#[test]
fn hostile_tables_are_refused() {
    let corner = Vec2::new(400.0, 280.0);
    let with = |change: &dyn Fn(&mut Table)| {
        let mut t = table();
        change(&mut t);
        t
    };
    let mut cases: Vec<(Table, &str)> = vec![
        (with(&|t| t.columns.clear()), "a table has 1 to 64 columns"),
        (with(&|t| t.columns = (0..65).map(|i| column(&format!("c{i}"), "C", HAlign::Left)).collect()), "a table has 1 to 64 columns"),
        (with(&|t| t.columns[1].name = "item".into()), "no two columns have the same name"),
        (with(&|t| t.columns[0].heading = "Item\nnumber".into()), "a label is one line of at most 4096 bytes"),
        (with(&|t| t.columns[0].content = Content(vec![Part::Text(String::new()); 65])), "a cell has at most 64 parts"),
        (
            with(&|t| t.columns[0].content = Content(vec![Part::Field { source: Source::Component, key: "name".into(), format: Format::default() }])),
            "a table's cells read their row",
        ),
        (
            with(&|t| {
                t.columns[0].content =
                    Content(vec![Part::Field { source: Source::Row, key: "mass".into(), format: Format { unit: Some("kg".into()), decimals: None } }])
            }),
            "a row field is shown as the row gives it",
        ),
        (
            with(&|t| {
                t.columns[0].content =
                    Content(vec![Part::Field { source: Source::Row, key: "mass".into(), format: Format { unit: None, decimals: Some(2) } }])
            }),
            "a row field is shown as the row gives it",
        ),
        (with(&|t| t.filter.text.push(TextFilter { column: "colour".into(), contains: "x".into() })), "a text filter names a column"),
        (with(&|t| t.filter.text = vec![TextFilter { column: "item".into(), contains: "x".into() }; 65]), "a table has at most 64 text filters"),
        (with(&|t| t.overrides.push(Override { row: "plate".into(), column: "colour".into(), text: "x".into() })), "an override names a column"),
        (
            with(&|t| t.overrides = vec![Override { row: "plate".into(), column: "item".into(), text: "x".into() }; 2]),
            "no two overrides are for one cell",
        ),
        (
            with(&|t| t.overrides.push(Override { row: "k".repeat(2049), column: "item".into(), text: "x".into() })),
            "a row's key is at most 2048 bytes",
        ),
        (
            with(&|t| t.overrides.push(Override { row: "plate".into(), column: "item".into(), text: "a\u{7}".into() })),
            "a cell's text has no control character but a line break",
        ),
        (
            with(&|t| t.overrides = (0..=MAX_ROWS).map(|i| Override { row: i.to_string(), column: "item".into(), text: String::new() }).collect()),
            "a table keeps at most 10,000 overrides",
        ),
    ];
    for name in ["", "Item", "1st", "part-number", "part number", "_item", "ïtem"] {
        cases.push((with(&|t| t.columns[0].name = name.into()), "a column's name is snake case and at most 64 characters"));
    }
    for height in [0.0, -7.0, 1000.001, f64::NAN, f64::INFINITY] {
        cases.push((with(&|t| t.row_height_mm = height), "a row's height is above 0 and at most 1000 mm"));
    }
    for cap in [0.0, -2.5, 7.000_001, f64::NAN] {
        cases.push((with(&|t| t.cap_mm = cap), "the text's cap height is above 0 and at most a row's height"));
    }
    for line in [0.0, -0.35, 10.000_001, f64::NAN] {
        cases.push((with(&|t| t.outline_line_mm = line), "a table's lines are above 0 and at most 10 mm wide"));
        cases.push((with(&|t| t.cell_line_mm = line), "a table's lines are above 0 and at most 10 mm wide"));
    }
    // Room for text: more than 2 mm of padding and the 0.7 mm outline.
    for width in [2.7, 0.0, -20.0, 5000.001, f64::NAN, f64::INFINITY] {
        cases.push((with(&|t| t.columns[0].width_mm = Some(width)), "a column's width leaves room for its text and is at most 5000 mm"));
    }
    // With cell lines wider than the outline, theirs is the width that counts: 2 mm and 1 mm.
    let wide_cells = |width: f64| {
        with(&|t| {
            t.cell_line_mm = 1.0;
            t.columns[0].width_mm = Some(width);
        })
    };
    cases.push((wide_cells(3.0), "a column's width leaves room for its text and is at most 5000 mm"));
    // 1 MiB of text in the definition: 257 overrides of 4096 bytes is 4 KiB past it with their keys.
    let big = "x".repeat(4096);
    cases.push((
        with(&|t| t.overrides = (0..257).map(|i| Override { row: i.to_string(), column: "item".into(), text: big.clone() }).collect()),
        "a table holds at most 1 MiB of text",
    ));
    for (table, message) in cases {
        assert_eq!(table.validate(), Err(Invalid(message)), "{message}");
        assert_eq!(table.layout(&rows(), corner, &fixed), Err(Invalid(message)), "{message}");
    }
    // And their limits.
    let accepted = [
        with(&|t| t.columns = (0..64).map(|i| column(&format!("c{i}"), "C", HAlign::Left)).collect()),
        with(&|t| t.columns[0].name = "a".repeat(64)),
        with(&|t| t.columns[0].width_mm = Some(2.7 + 1e-9)),
        wide_cells(3.0 + 1e-9),
        with(&|t| t.columns[0].width_mm = Some(5000.0)),
        with(&|t| t.cap_mm = 7.0),
        with(&|t| t.row_height_mm = 1000.0),
        with(&|t| t.outline_line_mm = 10.0),
        with(&|t| t.overrides = (0..MAX_ROWS).map(|i| Override { row: i.to_string(), column: "item".into(), text: String::new() }).collect()),
        with(&|t| t.overrides = (0..255).map(|i| Override { row: i.to_string(), column: "item".into(), text: big.clone() }).collect()),
    ];
    for table in accepted {
        table.validate().unwrap();
    }
}

#[test]
fn hostile_rows_and_measures_are_refused() {
    let corner = Vec2::new(400.0, 280.0);
    let t = table();
    let layout = |rows: &[Row]| t.layout(rows, corner, &fixed);
    let plain = |key: String| row(&key, RowKind::Part, false, &[("item", "1")]);
    let many: Vec<Row> = (0..=MAX_ROWS).map(|i| plain(i.to_string())).collect();
    assert_eq!(layout(&many), Err(Invalid("a table lists at most 10,000 rows")));
    assert_eq!(layout(&many[..MAX_ROWS]).unwrap().rows.len(), MAX_ROWS);
    assert_eq!(layout(&[plain("a".into()), plain("a".into())]), Err(Invalid("no two rows have the same key")));
    assert_eq!(layout(&[plain("k".repeat(2049))]), Err(Invalid("a row's key is at most 2048 bytes")));
    assert_eq!(layout(&[plain("k".repeat(2048))]).unwrap().rows.len(), 1);
    let keyed = row("a", RowKind::Part, false, &[(&"k".repeat(257), "1")]);
    assert_eq!(layout(&[keyed]), Err(Invalid("a row field's key is at most 256 bytes")));
    let bell = row("a", RowKind::Part, false, &[("item", "1\u{7}")]);
    assert_eq!(layout(&[bell]), Err(Invalid("a cell's text has no control character but a line break")));
    let long = row("a", RowKind::Part, false, &[("item", &"1".repeat(4097))]);
    assert_eq!(layout(&[long]), Err(Invalid("a cell's text is at most 4096 bytes")));
    // A column's own text repeated on every row counts: 4 KiB of text on 257 rows is past 1 MiB,
    // refused before it is built.
    let mut wide = table();
    wide.columns[0].content = Content(vec![Part::Text("x".repeat(4095))]);
    let repeated: Vec<Row> = (0..257).map(|i| plain(i.to_string())).collect();
    assert_eq!(wide.layout(&repeated, corner, &fixed), Err(Invalid("a table holds at most 1 MiB of text")));
    wide.layout(&repeated[..200], corner, &fixed).unwrap();
    for bad in [f64::NAN, f64::INFINITY, -1.0] {
        assert_eq!(t.layout(&rows(), corner, &|_, _| bad), Err(Invalid("a measured text is finite and at least 0 mm wide")), "{bad}");
    }
    for corner in [Vec2::new(f64::NAN, 0.0), Vec2::new(1e5 + 1.0, 0.0), Vec2::new(0.0, -1e5 - 1.0)] {
        assert_eq!(t.layout(&rows(), corner, &fixed), Err(Invalid("a table lies within the paper's coordinates")), "{corner:?}");
    }
    // Inside at the corner, outside once laid out: 28 mm of rows down from y = -99,990.
    assert_eq!(t.layout(&rows(), Vec2::new(0.0, -99_990.0), &fixed), Err(Invalid("a table lies within the paper's coordinates")));
    // No rows: the heading alone, on the outline both ways.
    let alone = t.layout(&[], corner, &fixed).unwrap();
    assert!(alone.rows.is_empty() && near(alone.outline, alone.heading.rect));
}

/// A paper group's strokes as each piece's ends in micrometres, closed or not, and width: what a
/// hand-worked list is compared with.
fn strokes(group: &Group) -> Vec<(Vec<[i64; 4]>, bool, f64)> {
    let um = |v: f64| (v * 1000.0).round() as i64;
    group
        .items
        .iter()
        .map(|item| match item {
            Item::Stroke { path, closed, width_mm, pattern: None } => {
                (path.iter().map(|s| [um(s.start().x), um(s.start().y), um(s.end().x), um(s.end().y)]).collect(), *closed, *width_mm)
            }
            other => panic!("{other:?}"),
        })
        .collect()
}

/// One straight piece from (x0, y0) to (x1, y1) mm, open, `width` wide.
fn piece(x0: f64, y0: f64, x1: f64, y1: f64, width: f64) -> (Vec<[i64; 4]>, bool, f64) {
    let um = |v: f64| (v * 1000.0).round() as i64;
    (vec![[um(x0), um(y0), um(x1), um(y1)]], false, width)
}

/// An outline round (x0, y0)..(x1, y1) mm, anticlockwise from its lower-left corner, 0.7 wide.
fn outline(x0: f64, y0: f64, x1: f64, y1: f64) -> (Vec<[i64; 4]>, bool, f64) {
    let um = |v: f64| (v * 1000.0).round() as i64;
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)];
    (corners.windows(2).map(|w| [um(w[0].0), um(w[0].1), um(w[1].0), um(w[1].1)]).collect(), true, 0.7)
}

/// By hand, from the layout above: the outline round 365.6..400 by 252..280; a 0.35 mm line across
/// at the lower edge of the heading, the plate and the pin (273, 266, 259), not the frame's, which
/// is the outline's; and one down at each column's right edge but the last (365.6 + 8.525, then
/// + 18.85). Each end is the layout's own corner, the same number exactly.
#[test]
fn a_tables_lines_on_paper_are_its_outline_and_each_inner_edge_once() {
    let layout = table().layout(&rows(), Vec2::new(400.0, 280.0), &fixed).unwrap();
    let group = layout.paper(9, ObjectKind::PartsList);
    assert_eq!((group.object, group.kind), (9, ObjectKind::PartsList));
    let across = |y| piece(365.6, y, 400.0, y, 0.35);
    let down = |x| piece(x, 252.0, x, 280.0, 0.35);
    assert_eq!(strokes(&group), [outline(365.6, 252.0, 400.0, 280.0), across(273.0), across(266.0), across(259.0), down(374.125), down(392.975)]);
    let Item::Stroke { path, .. } = &group.items[1] else { panic!("a stroke") };
    assert_eq!(
        (path[0].start(), path[0].end()),
        (Vec2::new(layout.outline.min.x, layout.heading.rect.min.y), Vec2::new(layout.outline.max.x, layout.heading.rect.min.y))
    );
    let Item::Stroke { path, .. } = &group.items[4] else { panic!("a stroke") };
    assert_eq!(path[0].start().x, layout.rows[2].cells[0].rect.max.x);
    sheet(vec![group]).validate().unwrap();
}

/// With the heading at the bottom the rows run up from it: the lines across are at the lower edge
/// of the frame, the pin and the plate (31, 24, 17), the heading's being the outline's.
#[test]
fn with_the_heading_at_the_bottom_the_lines_are_the_same_rule() {
    let table = Table { heading: Heading::Bottom, ..table() };
    let group = table.layout(&rows(), Vec2::new(400.0, 10.0), &fixed).unwrap().paper(3, ObjectKind::RevisionTable);
    assert_eq!((group.object, group.kind), (3, ObjectKind::RevisionTable));
    let across = |y| piece(365.6, y, 400.0, y, 0.35);
    let down = |x| piece(x, 10.0, x, 38.0, 0.35);
    assert_eq!(strokes(&group), [outline(365.6, 10.0, 400.0, 38.0), across(31.0), across(24.0), across(17.0), down(374.125), down(392.975)]);
    sheet(vec![group]).validate().unwrap();
}

/// The shapes: a filter that lets no row through leaves the heading alone (no line across); one
/// column has no line down; one row has one line across.
#[test]
fn a_heading_alone_and_one_column_have_only_the_lines_they_need() {
    let none = Table { filter: Filter { only: Some(Only::Assemblies), text: vec![] }, ..table() };
    let heading_alone = none.layout(&rows()[..2], Vec2::new(400.0, 280.0), &fixed).unwrap().paper(1, ObjectKind::PartsList);
    let down = |x| piece(x, 273.0, x, 280.0, 0.35);
    assert_eq!(strokes(&heading_alone), [outline(365.6, 273.0, 400.0, 280.0), down(374.125), down(392.975)]);
    let one = Table { columns: vec![column("item", "Item", HAlign::Centre)], ..table() };
    let layout = one.layout(&rows()[..1], Vec2::new(400.0, 280.0), &fixed).unwrap();
    // Item alone: 6 mm of text, 2 mm of padding and half the outline on each side: 8.7 mm.
    assert_eq!(strokes(&layout.paper(1, ObjectKind::PartsList)), [outline(391.3, 266.0, 400.0, 280.0), piece(391.3, 273.0, 400.0, 273.0, 0.35)]);
}

/// A table at its limits, 10,000 rows and 64 columns, is 10,064 strokes: within the paper scene's
/// own checks.
#[test]
fn a_table_at_its_limits_is_a_valid_paper_group() {
    let columns: Vec<Column> = (0..MAX_COLUMNS).map(|i| column(&format!("c{i}"), "C", HAlign::Left)).collect();
    let rows: Vec<Row> = (0..MAX_ROWS).map(|i| row(&i.to_string(), RowKind::Part, false, &[])).collect();
    let table = Table { columns, ..table() };
    let group = table.layout(&rows, Vec2::new(400.0, 280.0), &fixed).unwrap().paper(1, ObjectKind::PartsList);
    assert_eq!(group.items.len(), 1 + MAX_ROWS + MAX_COLUMNS - 1);
    sheet(vec![group]).validate().unwrap();
}

/// `n` parts, each with its item number, one part number and a quantity.
fn numbered(n: usize) -> Vec<Row> {
    (1..=n).map(|i| row(&format!("r{i}"), RowKind::Part, false, &[("item", &i.to_string()), ("part_number", "PL"), ("quantity", "1")])).collect()
}

fn split(rows: u32, gap_mm: f64, side: Side) -> Table {
    Table { split: Some(Split { rows, gap_mm, side }), ..table() }
}

/// The issue's case: 40 rows split at 20 stand in two parts of 20, each under its own heading, the
/// second a part's width and the gap left of the first, their headings in line. Each part draws its
/// own outline and lines, and its words are the heading's again.
#[test]
fn a_split_table_stands_in_parts_each_with_its_heading() {
    let layout = split(20, 5.0, Side::Left).layout(&numbered(40), Vec2::new(400.0, 280.0), &fixed).unwrap();
    assert_eq!(layout.parts.iter().map(|p| p.rows.clone()).collect::<Vec<_>>(), [0..20, 20..40]);
    let [first, second] = [&layout.parts[0], &layout.parts[1]];
    let moved = Vec2::new(first.outline.size().x + 5.0, 0.0);
    assert!(near(second.outline, Aabb2 { min: first.outline.min - moved, max: first.outline.max - moved }), "{:?}", second.outline);
    for part in [first, second] {
        assert!(at(part.heading.rect.max, part.outline.max.x, 280.0));
        assert_eq!(part.heading.cells.iter().map(texts).collect::<Vec<_>>(), [["Item"], ["Part number"], ["Qty"]]);
    }
    // Its first row just under its heading, and the first part's heading is the table's.
    assert_eq!(layout.rows[20].key.as_deref(), Some("r21"));
    assert!((layout.rows[20].rect.max.y - second.heading.rect.min.y).abs() < 1e-9);
    assert_eq!(layout.heading, first.heading);
    assert!(near(layout.outline, first.outline.union(&second.outline)));
    assert_eq!(layout.drawn_rows().count(), 42, "two headings and 40 rows");
    let group = layout.paper(1, ObjectKind::PartsList);
    let outlines = group.items.iter().filter(|item| matches!(item, Item::Stroke { closed: true, .. })).count();
    // Each part: its outline, 20 lines across and 2 down.
    assert_eq!((outlines, group.items.len()), (2, 2 * (1 + 20 + 2)));
    sheet(vec![group]).validate().unwrap();
}

/// With the heading at the bottom the parts grow up from their headings, in line; a part on the right
/// starts a part's width right of the one before; the last part holds what is left; a split longer
/// than the rows the filter lets through is one part, the same as no split; no row is one part.
#[test]
fn a_split_table_at_the_bottom_on_the_right_and_short() {
    let bottom = Table { heading: Heading::Bottom, ..split(2, 0.0, Side::Right) };
    let layout = bottom.layout(&numbered(5), Vec2::new(100.0, 20.0), &fixed).unwrap();
    assert_eq!(layout.parts.iter().map(|p| p.rows.len()).collect::<Vec<_>>(), [2, 2, 1]);
    let width = layout.parts[0].outline.size().x;
    for (k, part) in layout.parts.iter().enumerate() {
        assert!((part.heading.rect.min.y - 20.0).abs() < 1e-9, "part {k}: its heading in line");
        assert!((part.outline.max.x - (100.0 + k as f64 * width)).abs() < 1e-9, "part {k}: a part's width right of the one before");
        let first = &layout.rows[part.rows.start];
        assert!((first.rect.min.y - part.heading.rect.max.y).abs() < 1e-9, "part {k}: its first row just above its heading");
    }
    let long = split(20, 3.0, Side::Left).layout(&rows(), Vec2::new(400.0, 280.0), &fixed).unwrap();
    let plain = table().layout(&rows(), Vec2::new(400.0, 280.0), &fixed).unwrap();
    assert_eq!((long.parts.len(), long.outline, &long.rows), (1, plain.outline, &plain.rows));
    let none = Table { filter: Filter { only: Some(Only::Assemblies), text: vec![] }, ..split(1, 0.0, Side::Left) };
    let alone = none.layout(&rows()[..2], Vec2::new(400.0, 280.0), &fixed).unwrap();
    assert_eq!((alone.parts.len(), alone.parts[0].rows.clone(), alone.rows.len()), (1, 0..0, 0));
}

/// The split's rules at their bounds; parts that would pass the paper's coordinates, or more than 64
/// of them, are refused with a reason, never drawn; no split is never written, and one reads back as
/// it was saved.
#[test]
fn a_splits_rules_hold_at_their_bounds() {
    for (rows, gap, ok) in [
        (0, 0.0, false),
        (1, 0.0, true),
        (MAX_ROWS as u32, MAX_SIDE_MM, true),
        (MAX_ROWS as u32 + 1, 0.0, false),
        (1, -0.1, false),
        (1, MAX_SIDE_MM + 0.1, false),
        (1, f64::NAN, false),
        (1, f64::INFINITY, false),
    ] {
        assert_eq!(split(rows, gap, Side::Left).validate().is_ok(), ok, "rows {rows}, gap {gap}");
    }
    let far = split(1, MAX_SIDE_MM, Side::Left).layout(&numbered(40), Vec2::new(400.0, 280.0), &fixed).unwrap_err();
    assert!(far.0.contains("paper's coordinates"), "{far}");
    let many = split(1, 0.0, Side::Right).layout(&numbered(65), Vec2::new(0.0, 280.0), &fixed).unwrap_err();
    assert!(many.0.contains("at most 64 parts"), "{many}");
    assert_eq!(split(1, 0.0, Side::Right).layout(&numbered(64), Vec2::new(0.0, 280.0), &fixed).map(|l| l.parts.len()), Ok(64));
    assert!(serde_json::to_value(table()).unwrap().get("split").is_none());
    let saved = split(3, 2.5, Side::Right);
    assert_eq!(serde_json::from_value::<Table>(serde_json::to_value(&saved).unwrap()).unwrap(), saved);
    let read: Split = serde_json::from_value(json!({"rows": 3})).unwrap();
    assert_eq!(read, Split { rows: 3, gap_mm: 0.0, side: Side::Left });
    assert!(serde_json::from_value::<Split>(json!({"rows": 3, "gap": 1})).is_err());
}

/// Every small shape, both headings and both sides: parts hold at most the split's rows, in order,
/// every row once; each row lies in its part's outline; the parts are one width, the gap apart, and
/// their headings' outer edges in line.
#[test]
fn split_tables_of_every_small_shape_keep_their_parts_apart() {
    for n in 0..=7 {
        for per in 1..=4u32 {
            for heading in [Heading::Top, Heading::Bottom] {
                for side in [Side::Left, Side::Right] {
                    let table = Table { heading, ..split(per, 1.5, side) };
                    let layout = table.layout(&numbered(n), Vec2::new(200.0, 150.0), &fixed).unwrap();
                    let case = format!("{n} rows, {per} a part, {heading:?}, {side:?}");
                    assert_eq!(layout.parts.len(), n.div_ceil(per as usize).max(1), "{case}");
                    let mut next = 0;
                    for (k, part) in layout.parts.iter().enumerate() {
                        assert!(part.rows.start == next && part.rows.len() <= per as usize, "{case}: part {k}");
                        next = part.rows.end;
                        let edge = if heading == Heading::Top { part.outline.max.y } else { part.outline.min.y };
                        assert!((edge - 150.0).abs() < 1e-9, "{case}: part {k}'s heading in line");
                        assert!((part.outline.size().x - layout.parts[0].outline.size().x).abs() < 1e-9, "{case}");
                        for row in &layout.rows[part.rows.clone()] {
                            assert!(row.rect.min.y >= part.outline.min.y - 1e-9 && row.rect.max.y <= part.outline.max.y + 1e-9, "{case}");
                            assert!((row.rect.min.x - part.outline.min.x).abs() < 1e-9, "{case}");
                        }
                        if let Some(before) = k.checked_sub(1).and_then(|b| layout.parts.get(b)) {
                            let apart = match side {
                                Side::Left => before.outline.min.x - part.outline.max.x,
                                Side::Right => part.outline.min.x - before.outline.max.x,
                            };
                            assert!((apart - 1.5).abs() < 1e-9, "{case}: the gap");
                        }
                    }
                    assert_eq!(next, n, "{case}: every row once");
                    let keys: Vec<_> = layout.rows.iter().filter_map(|r| r.key.clone()).collect();
                    assert_eq!(keys, (1..=n).map(|i| format!("r{i}")).collect::<Vec<_>>(), "{case}: in order");
                }
            }
        }
    }
}

/// A part with these fields, `level` deep, with these numbers.
fn ranked(key: &str, level: u32, fields: &[(&str, &str)], numbers: &[(&str, f64)]) -> Row {
    Row { level, numbers: numbers.iter().map(|(k, n)| (k.to_string(), *n)).collect(), ..row(key, RowKind::Part, false, fields) }
}

/// These columns, sorted by `keys` (column, descending), the first first.
fn sorted_by(columns: Vec<Column>, keys: &[(&str, bool)]) -> Table {
    let sort = keys.iter().map(|(column, descending)| SortKey { column: column.to_string(), descending: *descending }).collect();
    Table { columns, sort, ..table() }
}

/// The keys of the rows a table shows, in order.
fn order_of(table: &Table, rows: &[Row]) -> Vec<String> {
    table.layout(rows, Vec2::new(400.0, 280.0), &fixed).unwrap().rows.iter().filter_map(|r| r.key.clone()).collect()
}

/// Text sorts without case, a run of digits as a number with its leading zeros left out; equal
/// cells keep the list's order whichever way; a missing value sorts after letters and digits.
#[test]
fn a_sort_orders_text_without_case_and_digit_runs_as_numbers() {
    let texts = [("k1", "A10"), ("k2", "a2"), ("k3", "007"), ("k4", "B1"), ("k5", "7")];
    let mut rows: Vec<Row> = texts.iter().map(|(key, text)| ranked(key, 0, &[("part_number", text)], &[])).collect();
    rows.push(ranked("k6", 0, &[], &[]));
    let columns = || vec![column("item", "Item", HAlign::Centre), column("part_number", "Part number", HAlign::Left)];
    assert_eq!(order_of(&sorted_by(columns(), &[("part_number", false)]), &rows), ["k3", "k5", "k2", "k1", "k4", "k6"]);
    assert_eq!(order_of(&sorted_by(columns(), &[("part_number", true)]), &rows), ["k6", "k4", "k1", "k2", "k3", "k5"]);
    assert_eq!(order_of(&sorted_by(columns(), &[]), &rows), ["k1", "k2", "k3", "k4", "k5", "k6"], "no sort: the list's order");
    for (a, b, found) in [("9", "10", Ordering::Less), ("a2", "a10", Ordering::Less), ("1.2", "1.10", Ordering::Less), ("x", "x0", Ordering::Less)] {
        assert_eq!(natural(a, b), found, "{a} / {b}");
        assert_eq!(natural(b, a), found.reverse(), "{b} / {a}");
    }
    let long = "9".repeat(400);
    assert_eq!(natural(&long, &format!("1{long}")), Ordering::Less, "a long run is compared by its length, never parsed");
}

/// A column of numbers sorts by the rows' numbers whatever their text's format: zeros trimmed, a
/// decimal comma, a minus sign. Text typed over a cell sorts by value when it reads as a number
/// (`0,3`, `−2`) and after every number when it doesn't (`about 2`); so does a row without one.
#[test]
fn a_column_of_numbers_sorts_by_value_whatever_its_format() {
    let rows = vec![
        ranked("half", 0, &[("mass", "0.5")], &[("mass", 0.5)]),
        ranked("quarter", 0, &[("mass", "0.25")], &[("mass", 0.25)]),
        ranked("eleven", 0, &[("mass", "1.1")], &[("mass", 1.1)]),
        ranked("comma", 0, &[("mass", "0,75")], &[("mass", 0.75)]),
        ranked("minus", 0, &[("mass", "-1.5")], &[("mass", -1.5)]),
        ranked("unknown", 0, &[], &[]),
    ];
    let columns = || vec![column("item", "Item", HAlign::Centre), column("mass", "Mass", HAlign::Right)];
    assert_eq!(order_of(&sorted_by(columns(), &[("mass", false)]), &rows), ["minus", "quarter", "half", "comma", "eleven", "unknown"]);
    assert_eq!(order_of(&sorted_by(columns(), &[("mass", true)]), &rows), ["eleven", "comma", "half", "quarter", "minus", "unknown"]);
    let typed = |row: &str, text: &str| Override { row: row.into(), column: "mass".into(), text: text.into() };
    let overrides = vec![typed("eleven", "0,3"), typed("half", "about 2"), typed("comma", "\u{2212}2")];
    let overridden = Table { overrides, ..sorted_by(columns(), &[("mass", false)]) };
    assert_eq!(order_of(&overridden, &rows), ["comma", "minus", "quarter", "eleven", "half", "unknown"]);
    for (text, read) in [
        ("0,5", Some(0.5)),
        (" -1.25 ", Some(-1.25)),
        ("+3", Some(3.0)),
        (".5", Some(0.5)),
        ("12 mm", None),
        ("1,234.5", None),
        ("-", None),
        ("1e3", None),
        ("", None),
    ] {
        assert_eq!(typed_number(text), read, "{text:?}");
    }
}

/// The item column reads as a path (1.2 before 1.10) and the mass column as numbers (1.10 before
/// 1.2): one text, two orders, each by what its column holds.
#[test]
fn an_item_sorts_as_a_path_and_a_mass_as_a_number() {
    let rows = vec![
        ranked("a", 0, &[("item", "1.10"), ("mass", "1.10")], &[("mass", 1.1)]),
        ranked("b", 0, &[("item", "1.2"), ("mass", "1.2")], &[("mass", 1.2)]),
    ];
    let columns = || vec![column("item", "Item", HAlign::Centre), column("mass", "Mass", HAlign::Right)];
    assert_eq!(order_of(&sorted_by(columns(), &[("item", false)]), &rows), ["b", "a"]);
    assert_eq!(order_of(&sorted_by(columns(), &[("mass", false)]), &rows), ["a", "b"]);
}

/// A second key orders rows the first finds equal, and rows equal on every key keep the list's
/// order. A column that isn't printed sorts too, and so does an override's text.
#[test]
fn a_second_key_breaks_ties_and_equal_rows_keep_their_order() {
    let rows = vec![
        ranked("r1", 0, &[("part_number", "B"), ("quantity", "2")], &[("quantity", 2.0)]),
        ranked("r2", 0, &[("part_number", "A"), ("quantity", "1")], &[("quantity", 1.0)]),
        ranked("r3", 0, &[("part_number", "B"), ("quantity", "1")], &[("quantity", 1.0)]),
        ranked("r4", 0, &[("part_number", "A"), ("quantity", "1")], &[("quantity", 1.0)]),
    ];
    let columns = || table().columns;
    assert_eq!(order_of(&sorted_by(columns(), &[("quantity", false), ("part_number", false)]), &rows), ["r2", "r4", "r3", "r1"]);
    assert_eq!(order_of(&sorted_by(columns(), &[("quantity", true), ("part_number", false)]), &rows), ["r1", "r2", "r4", "r3"]);
    let hidden = || {
        let mut columns = columns();
        columns[1].printed = false;
        columns
    };
    assert_eq!(order_of(&sorted_by(hidden(), &[("part_number", true)]), &rows), ["r1", "r3", "r2", "r4"]);
    let overrides = vec![Override { row: "r4".into(), column: "part_number".into(), text: "C".into() }];
    let typed = Table { overrides, ..sorted_by(columns(), &[("part_number", false)]) };
    assert_eq!(order_of(&typed, &rows), ["r2", "r1", "r3", "r4"]);
}

/// Each level sorts on its own: the rows under an assembly stay under it, in their own order. A row
/// listed deeper than the one before by more than one level is still under it. A filter that hides
/// an assembly leaves its rows where the sort put them.
#[test]
fn a_parts_list_sorts_within_each_level() {
    let named = |key: &str, level, name: &str| ranked(key, level, &[("part_number", name)], &[]);
    let rows = vec![
        named("A", 0, "b"),
        named("A.1", 1, "z"),
        named("A.2", 1, "y"),
        named("B", 0, "a"),
        named("B.1", 1, "x"),
        named("C", 0, "c"),
        named("C.1.1", 3, "w"),
        named("C.2", 1, "v"),
    ];
    let by_name = sorted_by(table().columns, &[("part_number", false)]);
    assert_eq!(order_of(&by_name, &rows), ["B", "B.1", "A", "A.2", "A.1", "C", "C.2", "C.1.1"]);
    let mut assemblies = rows.clone();
    for row in assemblies.iter_mut().filter(|r| r.level == 0) {
        row.kind = RowKind::Assembly;
    }
    let parts = Table { filter: Filter { only: Some(Only::Parts), text: vec![] }, ..by_name };
    assert_eq!(order_of(&parts, &assemblies), ["B.1", "A.2", "A.1", "C.2", "C.1.1"]);
}

/// The sort's rules at their bounds: 4 keys, not 5; each names one column of the table, once; no
/// sort and an ascending key are never written, and a saved sort reads back as it was. A row's
/// number is finite.
#[test]
fn a_sorts_rules_hold_at_their_bounds() {
    let columns: Vec<Column> = (0..5).map(|i| column(&format!("c{i}"), "C", HAlign::Left)).collect();
    let keys = |n: usize| (0..n).map(|i| SortKey { column: format!("c{i}"), descending: false }).collect::<Vec<_>>();
    assert!(Table { columns: columns.clone(), sort: keys(4), ..table() }.validate().is_ok());
    let refused = |t: Table| t.validate().unwrap_err().0;
    assert_eq!(refused(Table { columns: columns.clone(), sort: keys(5), ..table() }), "a table sorts by at most 4 columns");
    assert_eq!(refused(sorted_by(table().columns, &[("colour", false)])), "a sort key names a column");
    assert_eq!(refused(sorted_by(table().columns, &[("item", false), ("item", true)])), "no two sort keys name one column");
    assert!(serde_json::to_value(table()).unwrap().get("sort").is_none());
    let saved = sorted_by(table().columns, &[("quantity", true), ("item", false)]);
    let written = serde_json::to_value(&saved).unwrap();
    assert_eq!(written["sort"], json!([{"column": "quantity", "descending": true}, {"column": "item"}]));
    assert_eq!(serde_json::from_value::<Table>(written).unwrap(), saved);
    assert!(serde_json::from_value::<SortKey>(json!({"column": "item", "reverse": true})).is_err());
    for bad in [f64::NAN, f64::INFINITY] {
        let rows = vec![ranked("r", 0, &[], &[("quantity", bad)])];
        let found = table().layout(&rows, Vec2::new(400.0, 280.0), &fixed).unwrap_err();
        assert!(found.0.contains("is finite"), "{found}");
    }
}

/// Random lists, sorted by random keys: every row once; each row followed at once by every row
/// under it (by its list's levels, jumps too); rows under one parent in key order, and equal ones
/// in the list's order.
#[test]
fn sorted_tables_of_random_shapes_keep_each_row_with_its_own() {
    let mut seed = 0x1305_6502_5a17_c3d1u64;
    let mut next = |n: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % n.max(1)
    };
    let words = ["", "a", "A", "b", "1", "2", "10", "a2", "a10", "007", "7", "\u{2014}"];
    let names = ["item", "part_number", "quantity"];
    for round in 0..500 {
        let n = next(25) as usize;
        let mut level = 0u32;
        let mut rows = Vec::with_capacity(n);
        for i in 0..n {
            level = next(u64::from(level) + 3) as u32;
            let (word, number) = (words[next(words.len() as u64) as usize], next(3));
            let mut row = ranked(&format!("r{i}"), level, &[("part_number", word), ("quantity", &number.to_string())], &[]);
            if next(4) != 0 {
                row.numbers.insert("quantity".into(), number as f64);
            }
            rows.push(row);
        }
        let mut keys: Vec<(&str, bool)> = Vec::new();
        for _ in 0..1 + next(2) {
            let key = (names[next(3) as usize], next(2) == 1);
            if keys.iter().all(|(c, _)| *c != key.0) {
                keys.push(key);
            }
        }
        let sorted = sorted_by(table().columns, &keys);
        let at: HashMap<String, usize> = rows.iter().enumerate().map(|(i, r)| (r.key.clone(), i)).collect();
        let order: Vec<usize> = order_of(&sorted, &rows).iter().map(|k| at[k]).collect();
        let case = format!("round {round}: levels {:?}, keys {keys:?}", rows.iter().map(|r| r.level).collect::<Vec<_>>());
        let mut every = order.clone();
        every.sort_unstable();
        assert_eq!(every, (0..n).collect::<Vec<_>>(), "{case}: every row once");
        // Each row's parent: the nearest row before it listed less deep.
        let parent: Vec<Option<usize>> = (0..n).map(|i| (0..i).rev().find(|&j| rows[j].level < rows[i].level)).collect();
        let under = |i: usize, j: usize| {
            let mut p = parent[j];
            while let Some(q) = p {
                if q == i {
                    return true;
                }
                p = parent[q];
            }
            false
        };
        for (place, &i) in order.iter().enumerate() {
            let size = (0..n).filter(|&j| under(i, j)).count();
            assert!(order[place + 1..place + 1 + size].iter().all(|&j| under(i, j)), "{case}: the rows under r{i} follow it");
        }
        let overrides = Overrides::new();
        let sort_columns: Vec<(&Column, bool)> = keys.iter().map(|(c, d)| (sorted.columns.iter().find(|x| x.name == *c).unwrap(), *d)).collect();
        let value = |i: usize| {
            let mut budget = Budget(0);
            sort_columns.iter().map(|(c, _)| sort_value(&rows[i], c, &overrides, &mut budget).unwrap()).collect::<Vec<_>>()
        };
        for (place, &a) in order.iter().enumerate() {
            let Some(&b) = order[place + 1..].iter().find(|&&b| parent[b] == parent[a]) else { continue };
            let (x, y) = (value(a), value(b));
            let found = x.iter().zip(&y).zip(&sort_columns).fold(Ordering::Equal, |f, ((x, y), (_, d))| f.then_with(|| compare(x, y, *d)));
            assert!(found == Ordering::Less || (found == Ordering::Equal && a < b), "{case}: r{a} before r{b}");
        }
    }
}

/// An A3 landscape sheet with these groups.
fn sheet(groups: Vec<Group>) -> crate::paper::Scene {
    crate::paper::Scene::new(Vec2::new(420.0, 297.0), groups)
}
