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
    Column { name: name.into(), heading: heading.into(), content: field(name), width_mm: None, align }
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
    }
}

fn row(key: &str, kind: RowKind, purchased: bool, fields: &[(&str, &str)]) -> Row {
    Row { key: key.into(), kind, purchased, fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
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
    assert_eq!((texts(frame), frame.replaced.as_deref()), (vec![MISSING], None), "a field the row doesn't have");
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
        (with(&|t| t.overrides.push(Override { row: "k".repeat(257), column: "item".into(), text: "x".into() })), "a row's key is at most 256 bytes"),
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
    assert_eq!(layout(&[plain("k".repeat(257))]), Err(Invalid("a row's key is at most 256 bytes")));
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
            Item::Stroke { path, closed, width_mm } => {
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

/// An A3 landscape sheet with these groups.
fn sheet(groups: Vec<Group>) -> crate::paper::Scene {
    crate::paper::Scene { size_mm: Vec2::new(420.0, 297.0), groups }
}
