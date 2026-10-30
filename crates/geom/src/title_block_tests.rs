use serde_json::json;

use super::*;
use crate::bspline::tests::Lcg;
use crate::cell::{DESCENDER_OF_CAP, Format, PADDING_MM, Part, Source, Warning, place_lines};
use crate::drawing_sheet::{DrawingBorderProfile, Orientation, PaperSize, Sheet, SheetSize};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Aabb2 {
    Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) }
}

fn field(source: Source, key: &str) -> Part {
    Part::Field { source, key: key.to_string(), format: Format::default() }
}

fn text(cap_mm: f64, align: HAlign, fit: Fit, parts: Vec<Part>) -> CellKind {
    CellKind::Text(TextCell { content: Content(parts), cap_mm, align, fit })
}

fn sheet_layout(profile: &DrawingBorderProfile, size: PaperSize, orientation: Orientation) -> SheetLayout {
    profile.layout(&Sheet::standard(size, orientation)).unwrap()
}

fn iso(size: PaperSize, orientation: Orientation) -> SheetLayout {
    sheet_layout(&DrawingBorderProfile::iso(), size, orientation)
}

fn filing(size: PaperSize, orientation: Orientation) -> SheetLayout {
    sheet_layout(&DrawingBorderProfile { filing_margin: true, ..DrawingBorderProfile::iso() }, size, orientation)
}

fn cell_named<'a>(block: &TitleBlock, layout: &'a TitleBlockLayout, name: &str) -> &'a CellLayout {
    let i = block.cells.iter().position(|c| c.name == name).unwrap();
    &layout.cells[i]
}

/// `a` and `b` within 1e-9 mm at both corners.
fn near(a: Aabb2, b: Aabb2) -> bool {
    a.min.dist(b.min) < 1e-9 && a.max.dist(b.max) < 1e-9
}

/// `cell`'s rectangle placed at `corner`, inside its lines by the module's rule worked out by hand:
/// half the 0.7 mm outline on the block's edges, half the 0.35 mm cell line inside.
fn inside(cell: &Cell, corner: Vec2) -> Aabb2 {
    let [x0, y0, x1, y1] = cell.rect_mm;
    let half = |outline: bool| if outline { 0.35 } else { 0.175 };
    Aabb2 {
        min: corner + Vec2::new(x0 + half(x0 == -180.0), y0 + half(y0 == 0.0)),
        max: corner + Vec2::new(x1 - half(x1 == 0.0), y1 - half(y1 == 54.0)),
    }
}

fn text_of(cell: &Cell) -> &TextCell {
    match &cell.kind {
        CellKind::Text(text) => text,
        CellKind::ProjectionSymbol => panic!("{} has no text", cell.name),
    }
}

#[test]
fn the_default_geometry_validates_and_its_cells_tile_the_block() {
    let block = default_block();
    block.validate().unwrap();
    assert_eq!(block.cells.len(), 12);
    let area: f64 = block.cells.iter().map(|c| c.rect(Vec2::ZERO).size()).map(|s| s.x * s.y).sum();
    assert_eq!((area, 180.0 * 54.0), (9720.0, 9720.0));
    // Each text cell's value box holds one line of its cap height, centred, its descenders above the
    // lines' inner edge.
    for cell in &block.cells {
        let room = cell.value_box(block.inside_lines(cell, cell.rect(Vec2::ZERO))).size();
        if let CellKind::Text(text) = &cell.kind {
            let below = (room.y - text.cap_mm) / 2.0 + PADDING_MM;
            assert!(text.cap_mm <= room.y && text.cap_mm * DESCENDER_OF_CAP <= below, "{}: {} in {room:?}", cell.name, text.cap_mm);
        }
    }
}

#[test]
fn on_a3_landscape_the_block_sits_on_the_drawing_borders_lower_right_corner() {
    let (block, sheet) = (default_block(), iso(PaperSize::A3, Orientation::Landscape));
    let layout = block.layout(&sheet).unwrap();
    assert_eq!(layout.outline, rect(230.0, 10.0, 410.0, 64.0));
    assert_eq!(layout.outline.size().x, 180.0);
    assert_eq!((layout.outline_line_mm, layout.cell_line_mm), (0.7, 0.35));
    assert_eq!(layout.cells.len(), 12);
    // The title cell, on the outline at its left and bottom (0.35 mm in), a cell line at its right
    // and top (0.175 in): its label's baseline 1 + 2.5 mm below its top's inner edge, its value box
    // 1 mm in from the lines' inner edges and 1 mm under the label.
    let title = cell_named(&block, &layout, "title");
    assert_eq!(title.rect, rect(230.0, 10.0, 342.0, 24.0));
    assert!(title.label_at.is_some_and(|at| at.dist(Vec2::new(231.35, 20.325)) < 1e-9), "{:?}", title.label_at);
    assert!(near(title.value_box, rect(231.35, 11.35, 340.825, 19.325)), "{:?}", title.value_box);
    let (at, warning) = place_lines(title.value_box, 1, 5.0, 8.0, HAlign::Left, false).unwrap();
    assert!(at.dist(Vec2::new(231.35, 12.8375)) < 1e-9 && warning.is_none(), "{at:?}");
    // The projection cell, on the outline at its right and top.
    let projection = cell_named(&block, &layout, "projection");
    assert_eq!(projection.rect, rect(380.0, 44.0, 410.0, 64.0));
    assert!(projection.label_at.is_some_and(|at| at.dist(Vec2::new(381.175, 60.15)) < 1e-9), "{:?}", projection.label_at);
    assert!(near(projection.value_box, rect(381.175, 45.175, 408.65, 59.15)), "{:?}", projection.value_box);
    // A cell without a label gives its value the whole cell inside its lines, padded.
    let mut bare = block.clone();
    bare.cells[0].label = None;
    let title = bare.layout(&sheet).unwrap().cells[0];
    assert!(title.label_at.is_none() && near(title.value_box, rect(231.35, 11.35, 340.825, 22.825)), "{:?}", title.value_box);
}

/// Padding is measured from a line's inner edge: a 3 mm outline moves the title's text 1.5 mm in
/// from where a hairline puts it, so the line never covers it. The drawing border's line is a
/// hairline too, so only the outline moves the text.
#[test]
fn the_padding_starts_at_the_lines_inner_edge() {
    let sheet = SheetLayout { drawing_border_line_mm: 1e-9, ..iso(PaperSize::A3, Orientation::Landscape) };
    let hairline = TitleBlock { outline_line_mm: 1e-9, cell_line_mm: 1e-9, ..default_block() }.layout(&sheet).unwrap();
    let wide = TitleBlock { outline_line_mm: 3.0, ..default_block() }.layout(&sheet).unwrap();
    let (thin, thick) = (hairline.cells[0].value_box, wide.cells[0].value_box);
    assert!((thick.min.x - thin.min.x - 1.5).abs() < 1e-6 && (thick.min.y - thin.min.y - 1.5).abs() < 1e-6, "{thin:?} {thick:?}");
    // The outline's inner edge is 1.5 mm inside the cell's left side; the text starts 1 mm past it.
    assert!((thick.min.x - (230.0 + 1.5 + 1.0)).abs() < 1e-9, "{thick:?}");
}

/// The block's right and bottom edges lie on the drawing border, its line centred on them too: where
/// that line is wider than the outline, the cells there are padded from its inner edge instead.
#[test]
fn a_drawing_border_line_wider_than_the_outline_pads_the_right_and_bottom_cells() {
    let a3 = |line_mm: f64| SheetLayout { drawing_border_line_mm: line_mm, ..iso(PaperSize::A3, Orientation::Landscape) };
    let block = default_block();
    let boxes = |block: &TitleBlock, line_mm: f64| {
        let layout = block.layout(&a3(line_mm)).unwrap();
        ["title", "identification_number", "projection"].map(|name| cell_named(block, &layout, name).value_box)
    };
    // ISO's 0.7 mm, the outline's width, and a narrower line leave the outline's inner edge.
    assert_eq!(boxes(&block, 0.35), boxes(&block, 0.7));
    // A 2 mm line's inner edge is at y = 11 and x = 409: the value boxes start 1 mm above it and end
    // 1 mm left of it; the block's left and top edges keep the outline's.
    let [title, number, projection] = boxes(&block, 2.0);
    assert!(near(title, rect(231.35, 12.0, 340.825, 19.325)), "{title:?}");
    assert!(near(number, rect(343.175, 12.0, 408.0, 19.325)), "{number:?}");
    assert!(near(projection, rect(381.175, 45.175, 408.0, 59.15)), "{projection:?}");
    // A 5 mm line's inner edge is at y = 12.5 and x = 407.5: the title's 5 mm line no longer fits
    // above it, so the block that validates on its own is refused on this sheet. With 3.5 mm values,
    // the boxes keep 1 mm from it.
    block.validate().unwrap();
    assert_eq!(block.layout(&a3(5.0)), Err(Invalid("a value's cap height is above 0 and one line of it fits its value box")));
    let mut smaller = block.clone();
    for cell in &mut smaller.cells[..2] {
        if let CellKind::Text(text) = &mut cell.kind {
            text.cap_mm = 3.5;
        }
    }
    let [title, number, _] = boxes(&smaller, 5.0);
    assert!(near(title, rect(231.35, 13.5, 340.825, 19.325)) && near(number, rect(343.175, 13.5, 406.5, 19.325)), "{title:?} {number:?}");
    // A block that fills the drawing border has its left and top edges on the border's line too:
    // on a 200 x 74 sheet the border is (10, 10)-(190, 64), and a 2 mm line's inner edge is at x = 11
    // and y = 63.
    let sheet = |sides_mm| Sheet { size: SheetSize::Custom { sides_mm }, orientation: Orientation::Landscape };
    let filled = SheetLayout { drawing_border_line_mm: 2.0, ..DrawingBorderProfile::iso().layout(&sheet([200.0, 74.0])).unwrap() };
    let layout = block.layout(&filled).unwrap();
    let (title, projection) = (cell_named(&block, &layout, "title"), cell_named(&block, &layout, "projection"));
    assert!(near(title.value_box, rect(12.0, 12.0, 120.825, 19.325)), "{:?}", title.value_box);
    assert!(projection.label_at.is_some_and(|at| at.dist(Vec2::new(161.175, 59.5)) < 1e-9), "{:?}", projection.label_at);
    assert!(near(projection.value_box, rect(161.175, 45.175, 188.0, 58.5)), "{:?}", projection.value_box);
}

/// A cell with a few ulps of room at the origin, where a definition is checked on its own, has none
/// where A3's corner (x = 410) rounds it: the block is refused there, not laid out inside out, and
/// placed where rounding leaves the room (A2's corner, x = 584).
#[test]
fn a_cells_room_is_checked_again_where_the_block_sits() {
    let sliver = Cell { rect_mm: [-70.34999999999971, 0.0, -67.9999999999997, 14.0], ..default_block().cells[0].clone() };
    let block = TitleBlock { cells: vec![sliver], ..default_block() };
    block.validate().unwrap();
    const NO_ROOM: Invalid = Invalid("a cell has room for its value below its label");
    assert_eq!(block.layout(&iso(PaperSize::A3, Orientation::Landscape)), Err(NO_ROOM));
    let value_box = block.layout(&iso(PaperSize::A2, Orientation::Landscape)).unwrap().cells[0].value_box;
    assert!(value_box.min.x < value_box.max.x, "{value_box:?}");
}

#[test]
fn the_block_follows_the_drawing_border_on_a2_and_a4() {
    let block = default_block();
    assert_eq!(block.layout(&iso(PaperSize::A2, Orientation::Landscape)).unwrap().outline, rect(404.0, 10.0, 584.0, 64.0));
    // A4 upright: 10 mm to spare on the left of the 190 mm drawing border, none with the filing margin.
    let a4 = iso(PaperSize::A4, Orientation::Portrait);
    assert_eq!(a4.drawing_border, rect(10.0, 10.0, 200.0, 287.0));
    assert_eq!(block.layout(&a4).unwrap().outline, rect(20.0, 10.0, 200.0, 64.0));
    let a4 = filing(PaperSize::A4, Orientation::Portrait);
    let outline = block.layout(&a4).unwrap().outline;
    assert_eq!((outline.min.x, outline), (a4.drawing_border.min.x, rect(20.0, 10.0, 200.0, 64.0)));
}

#[test]
fn a5_landscape_holds_the_block_and_a5_portrait_is_too_narrow() {
    const TOO_BIG: Invalid = Invalid("a title block fits inside the drawing border");
    let block = default_block();
    assert_eq!(block.layout(&iso(PaperSize::A5, Orientation::Landscape)).unwrap().outline, rect(20.0, 10.0, 200.0, 64.0));
    // With the filing margin, the drawing border is exactly the block's width.
    let a5 = filing(PaperSize::A5, Orientation::Landscape);
    assert_eq!(a5.drawing_border.size().x, 180.0);
    assert_eq!(block.layout(&a5).unwrap().outline, rect(20.0, 10.0, 200.0, 64.0));
    // Upright, the drawing border is 128 mm wide, or 118 with the filing margin.
    for sheet in [iso(PaperSize::A5, Orientation::Portrait), filing(PaperSize::A5, Orientation::Portrait)] {
        assert!(sheet.drawing_border.size().x < 180.0);
        assert_eq!(block.layout(&sheet), Err(TOO_BIG));
    }
    // A drawing border of exactly 180 x 54 holds it; a hair less either way doesn't.
    let custom = |a: f64, b: f64| {
        DrawingBorderProfile::iso().layout(&Sheet { size: SheetSize::Custom { sides_mm: [a, b] }, orientation: Orientation::Landscape })
    };
    let exact = custom(200.0, 74.0).unwrap();
    assert_eq!(block.layout(&exact).unwrap().outline, exact.drawing_border);
    for sheet in [custom(199.999, 74.0), custom(200.0, 73.999)] {
        assert_eq!(block.layout(&sheet.unwrap()), Err(TOO_BIG));
    }
}

/// On every standard sheet either way, with the filing margin or without, the block sits on the
/// drawing border's lower-right corner and each cell keeps its place from it; only A5 upright is
/// too narrow.
#[test]
fn on_every_sheet_each_cell_keeps_its_place_from_the_corner() {
    let block = default_block();
    let mut refused = Vec::new();
    for size in PaperSize::ALL {
        for orientation in [Orientation::Landscape, Orientation::Portrait] {
            for sheet in [iso(size, orientation), filing(size, orientation)] {
                let border = sheet.drawing_border;
                let Ok(layout) = block.layout(&sheet) else {
                    assert!(border.size().x < 180.0 || border.size().y < 54.0, "{size:?} {orientation:?}");
                    refused.push((size, orientation));
                    continue;
                };
                let corner = Vec2::new(border.max.x, border.min.y);
                assert_eq!(layout.outline, Aabb2 { min: corner - Vec2::new(180.0, 0.0), max: corner + Vec2::new(0.0, 54.0) });
                for (cell, placed) in block.cells.iter().zip(&layout.cells) {
                    let [x0, y0, x1, y1] = cell.rect_mm;
                    let expected = Aabb2 { min: corner + Vec2::new(x0, y0), max: corner + Vec2::new(x1, y1) };
                    assert_eq!(placed.rect, expected, "{size:?} {orientation:?} {}", cell.name);
                    let lines = inside(cell, corner);
                    let label_at = Vec2::new(lines.min.x + 1.0, lines.max.y - 3.5);
                    assert!(placed.label_at.is_some_and(|at| at.dist(label_at) < 1e-9), "{}", cell.name);
                    let value_box = Aabb2 { min: lines.min + Vec2::new(1.0, 1.0), max: Vec2::new(lines.max.x - 1.0, lines.max.y - 4.5) };
                    assert!(near(placed.value_box, value_box), "{}", cell.name);
                }
            }
        }
    }
    assert_eq!(refused, [(PaperSize::A5, Orientation::Portrait); 2]);
}

/// The plan's line counts for the two wrapped cells at Liberation Sans' pitch (2355/1409 of the cap
/// height): two lines of 3.5 mm for the legal owner, three of 2.5 mm for the general tolerances.
#[test]
fn the_wrapped_cells_hold_the_lines_the_plan_gives_them() {
    let block = default_block();
    let layout = block.layout(&iso(PaperSize::A3, Orientation::Landscape)).unwrap();
    for (name, lines) in [("legal_owner", 2), ("general_tolerances", 3)] {
        let i = block.cells.iter().position(|c| c.name == name).unwrap();
        let text = text_of(&block.cells[i]);
        let pitch = text.cap_mm * 2355.0 / 1409.0;
        let place = |n| place_lines(layout.cells[i].value_box, n, text.cap_mm, pitch, text.align, text.fit.wrap).unwrap();
        // From the value box's top, 59.15 mm up the sheet: under the 0.7 mm outline's inner edge.
        let (at, warning) = place(lines);
        assert!(at.dist(Vec2::new(layout.cells[i].value_box.min.x, 59.15 - text.cap_mm)) < 1e-9 && warning.is_none(), "{name}: {at:?}");
        assert_eq!(place(lines + 1).1, Some(Warning::Height), "{name}");
    }
}

#[test]
fn hostile_definitions_are_refused() {
    let a3 = iso(PaperSize::A3, Orientation::Landscape);
    let with = |change: &dyn Fn(&mut TitleBlock)| {
        let mut block = default_block();
        change(&mut block);
        block
    };
    let named = |name: &'static str| move |b: &mut TitleBlock| b.cells[0].name = name.into();
    let at = |rect_mm: [f64; 4]| move |b: &mut TitleBlock| b.cells[0].rect_mm = rect_mm;
    let label_cap = |cap_mm: f64| move |b: &mut TitleBlock| b.cells[0].label = Some(Label { text: "Title".into(), cap_mm });
    let label_text = |text: String| move |b: &mut TitleBlock| b.cells[0].label = Some(Label { text: text.clone(), cap_mm: 2.5 });
    let value_cap = |cap_mm: f64| move |b: &mut TitleBlock| b.cells[0].kind = text(cap_mm, HAlign::Left, Fit::default(), vec![]);
    let mut cases: Vec<(TitleBlock, &str)> = Vec::new();
    for side in [0.999, 1000.001, 0.0, -180.0, f64::NAN, f64::INFINITY] {
        cases.push((with(&|b| b.size_mm.x = side), "each side of a title block is 1 to 1000 mm"));
        cases.push((with(&|b| b.size_mm.y = side), "each side of a title block is 1 to 1000 mm"));
    }
    for line in [0.0, -0.7, 10.000_001, f64::NAN, f64::INFINITY] {
        cases.push((with(&|b| b.outline_line_mm = line), "a title block's lines are above 0 and at most 10 mm wide"));
        cases.push((with(&|b| b.cell_line_mm = line), "a title block's lines are above 0 and at most 10 mm wide"));
    }
    let (longest, too_long): (&'static str, &'static str) = ("a".repeat(64).leak(), "a".repeat(65).leak());
    for name in ["", "Title", "title_Block", "1st", "title-block", "title block", "_title", "titlé", too_long] {
        cases.push((with(&named(name)), "a cell's name is snake case and at most 64 characters"));
    }
    cases.push((with(&|b| b.cells[1].name = "title".into()), "no two cells have the same name"));
    for rect_mm in [
        [-180.0, 0.0, 0.001, 14.0],
        [-180.0, -0.001, -68.0, 14.0],
        [-180.001, 0.0, -68.0, 14.0],
        [-180.0, 40.0, -68.0, 54.001],
        [-68.0, 0.0, -68.0, 14.0],
        [-180.0, 14.0, -68.0, 0.0],
        [f64::NAN, 0.0, -68.0, 14.0],
        [-180.0, 0.0, -68.0, f64::INFINITY],
    ] {
        cases.push((with(&at(rect_mm)), "a cell lies inside its block"));
    }
    // The identification number reaching 1 mm into the title, a copy of the title, a cell inside the
    // material cell.
    cases.push((with(&|b| b.cells[1].rect_mm = [-69.0, 0.0, 0.0, 14.0]), "no two cells overlap"));
    cases.push((with(&|b| b.cells.push(Cell { name: "copy".into(), ..b.cells[0].clone() })), "no two cells overlap"));
    cases.push((
        with(&|b| b.cells.push(Cell { name: "inner".into(), rect_mm: [-170.0, 26.0, -150.0, 32.0], ..b.cells[0].clone() })),
        "no two cells overlap",
    ));
    for cap in [0.0, -2.5, f64::NAN, f64::INFINITY] {
        cases.push((with(&label_cap(cap)), "a label's cap height is finite and above 0"));
    }
    for text in ["Title\nblock".to_string(), "\t".into(), "a".repeat(4097)] {
        cases.push((with(&label_text(text)), "a label is one line of at most 4096 bytes"));
    }
    // A 14 mm cell under a 12 mm label, a 2 mm wide cell, a symbol under a label filling its cell:
    // the projection cell is 19.475 mm high inside its lines, so a label of 16.475 leaves no room.
    cases.push((with(&label_cap(12.0)), "a cell has room for its value below its label"));
    cases.push((
        with(&|b| b.cells[11].label = Some(Label { text: "Projection".into(), cap_mm: 16.475 + 1e-9 })),
        "a cell has room for its value below its label",
    ));
    cases.push((with(&at([-180.0, 0.0, -178.0, 14.0])), "a cell has room for its value below its label"));
    cases.push((
        with(&|b| b.cells[11].label = Some(Label { text: "Projection".into(), cap_mm: 17.0 })),
        "a cell has room for its value below its label",
    ));
    // The title's value box is 7.975 mm high inside its lines. A line filling it has its descenders
    // 2.39 mm below the box, past the 1 mm padding; the most it holds centred is 9.975 / 1.6 mm, its
    // descenders (3/10 of it) then on the outline's inner edge.
    let title_most = 9.975 / 1.6;
    for cap in [0.0, -5.0, 7.975, title_most + 1e-6, f64::NAN, f64::INFINITY] {
        cases.push((with(&value_cap(cap)), "a value's cap height is above 0 and one line of it fits its value box"));
    }
    // The cell content's own rule, under this module's name.
    cases.push((
        with(&|b| b.cells[0].kind = text(5.0, HAlign::Left, Fit::default(), vec![Part::Text(String::new()); 65])),
        "a cell has at most 64 parts",
    ));
    let control = vec![Part::Text("a\u{0}".into())];
    cases.push((
        with(&|b| b.cells[0].kind = text(5.0, HAlign::Left, Fit::default(), control.clone())),
        "a cell's text has no control character but a line break",
    ));
    // A sheet table's row source, which a title block doesn't have, even beside other parts.
    let row = vec![field(Source::Component, "name"), Part::Text(" ".into()), field(Source::Row, "item")];
    cases.push((with(&|b| b.cells[0].kind = text(5.0, HAlign::Left, Fit::default(), row.clone())), "a title block's cells have no row to read"));
    let many = |n: usize| {
        let cell = |i: usize| Cell {
            name: format!("c{i}"),
            rect_mm: [-10.0 * (i + 1) as f64, 0.0, -10.0 * i as f64, 10.0],
            label: None,
            kind: text(1.0, HAlign::Left, Fit::default(), vec![]),
        };
        TitleBlock { size_mm: Vec2::new(1000.0, 10.0), outline_line_mm: 0.7, cell_line_mm: 0.35, cells: (0..n).map(cell).collect() }
    };
    cases.push((many(MAX_CELLS + 1), "a title block has at most 64 cells"));
    for (block, message) in cases {
        assert_eq!(block.validate(), Err(Invalid(message)), "{block:?}");
        assert_eq!(block.layout(&a3), Err(Invalid(message)), "{block:?}");
    }
    // And their limits are accepted.
    let accepted = [
        many(MAX_CELLS),
        TitleBlock { size_mm: Vec2::new(1.0, 1.0), cells: vec![], ..default_block() },
        TitleBlock { size_mm: Vec2::new(1000.0, 1000.0), cells: vec![], ..default_block() },
        // The widest outline on its own: the default's text doesn't fit inside a 10 mm line.
        TitleBlock { outline_line_mm: 10.0, cells: vec![], ..default_block() },
        with(&|b| b.cell_line_mm = 1e-9),
        with(&named("a")),
        with(&named("title_2")),
        with(&named(longest)),
        with(&value_cap(title_most - 1e-6)),
        with(&|b| b.cells[11].label = Some(Label { text: "Projection".into(), cap_mm: 16.475 - 1e-9 })),
        with(&label_text("a".repeat(4096))),
        with(&|b| b.cells[0].label = None),
    ];
    for block in accepted {
        block.validate().unwrap();
    }
}

#[test]
fn a_block_round_trips_through_json_and_unknown_fields_are_refused() {
    let block = default_block();
    let value = serde_json::to_value(&block).unwrap();
    assert_eq!(value["size_mm"], json!([180.0, 54.0]));
    assert_eq!((value["outline_line_mm"].clone(), value["cell_line_mm"].clone()), (json!(0.7), json!(0.35)));
    assert_eq!(
        value["cells"][5],
        json!({
            "name": "sheet",
            "rect_mm": [-31.0, 14.0, 0.0, 24.0],
            "label": {"text": "Sheet", "cap_mm": 2.5},
            "kind": {"text": {
                "content": [
                    {"field": {"source": "sheet", "key": "number", "format": {"unit": null, "decimals": null}}},
                    {"text": "/"},
                    {"field": {"source": "sheet", "key": "count", "format": {"unit": null, "decimals": null}}}
                ],
                "cap_mm": 3.5,
                "align": "centre",
                "fit": {"max_chars": 9, "wide_max_chars": null, "wrap": false}
            }}
        })
    );
    assert_eq!(value["cells"][11]["kind"], json!("projection_symbol"));
    let read = |v: serde_json::Value| serde_json::from_value::<TitleBlock>(v);
    assert_eq!(read(value.clone()).unwrap(), block);
    // Not given: no label, left-aligned, nothing to fit.
    let mut plain = value.clone();
    for key in ["align", "fit"] {
        plain["cells"][0]["kind"]["text"].as_object_mut().unwrap().remove(key);
    }
    plain["cells"][0].as_object_mut().unwrap().remove("label");
    let plain = read(plain).unwrap();
    assert_eq!((plain.cells[0].label.clone(), text_of(&plain.cells[0]).align, text_of(&plain.cells[0]).fit), (None, HAlign::Left, Fit::default()));
    for at in ["", "/cells/0", "/cells/0/label", "/cells/0/kind/text", "/cells/0/kind/text/fit"] {
        let mut v = value.clone();
        v.pointer_mut(at).unwrap()["scale"] = json!(1);
        assert!(read(v).is_err(), "an unknown field at {at:?}");
    }
    for (at, bad) in [
        ("/cells/11/kind", json!("logo")),
        ("/cells/11/kind", json!("projection")),
        ("/cells/11/kind", json!({"projection_symbol": {"angle": "third"}})),
        ("/cells/11/kind", json!({"kind": "projection_symbol"})),
        ("/cells/0/rect_mm", json!([-180.0, 0.0, -68.0])),
        ("/cells/0/rect_mm", json!({"x0": -180.0})),
        ("/cells/0/name", json!(1)),
        ("/size_mm", json!([180.0])),
    ] {
        let mut v = value.clone();
        *v.pointer_mut(at).unwrap() = bad.clone();
        assert!(read(v).is_err(), "{at}: {bad}");
    }
}

#[test]
fn a_drawing_border_that_isnt_finite_holds_no_block() {
    let block = default_block();
    let a3 = iso(PaperSize::A3, Orientation::Landscape);
    for drawing_border in [Aabb2::EMPTY, rect(410.0, 10.0, 10.0, 287.0), rect(f64::NAN, 10.0, 410.0, 287.0), rect(10.0, 10.0, f64::INFINITY, 287.0)] {
        let sheet = SheetLayout { drawing_border, ..a3.clone() };
        assert_eq!(block.layout(&sheet), Err(Invalid("a title block fits inside the drawing border")), "{drawing_border:?}");
    }
}

/// Random changes to the default's numbers, hostile ones among them: a definition is accepted or
/// refused without panicking, and an accepted one placed on a sheet it fits (any standard size,
/// either way, with or without the filing margin, any drawing border line) keeps every cell inside
/// the outline, apart from the others, with its label and value box inside it, and every value box
/// padded from the drawing border's line.
#[test]
fn fuzzed_definitions_never_panic_and_an_accepted_one_lays_out_inside_its_outline() {
    const NUMBERS: [f64; 9] = [0.0, -0.0, 1e-9, 1e5, -1e5, 1000.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
    let mut rng = Lcg(0x7b1c);
    let (mut accepted, mut laid_out) = (0, 0);
    for _ in 0..4000 {
        let mut block = default_block();
        for _ in 0..1 + (rng.unit() * 3.0) as usize {
            let i = (rng.unit() * 12.0) as usize % 12;
            let n = if rng.unit() < 0.3 { NUMBERS[(rng.unit() * 9.0) as usize % 9] } else { (rng.unit() - 0.5) * 20.0 };
            let cell = &mut block.cells[i];
            match (rng.unit() * 8.0) as usize {
                0 => block.size_mm.x += n,
                1 => block.size_mm.y = n,
                2 => block.outline_line_mm = n,
                3 => {
                    if let CellKind::Text(text) = &mut cell.kind {
                        text.cap_mm += n;
                    }
                }
                4 => cell.label = Some(Label { text: "x".into(), cap_mm: n }),
                k => cell.rect_mm[(k + i) % 4] += n,
            }
        }
        if block.validate().is_err() {
            continue;
        }
        accepted += 1;
        let size = PaperSize::ALL[(rng.unit() * PaperSize::ALL.len() as f64) as usize % PaperSize::ALL.len()];
        let orientation = if rng.unit() < 0.5 { Orientation::Landscape } else { Orientation::Portrait };
        let sheet = if rng.unit() < 0.5 { iso(size, orientation) } else { filing(size, orientation) };
        let line_mm = [0.25, 0.7, 2.0, 5.0, 10.0][(rng.unit() * 5.0) as usize % 5];
        let sheet = SheetLayout { drawing_border_line_mm: line_mm, ..sheet };
        let Ok(layout) = block.layout(&sheet) else { continue };
        laid_out += 1;
        let outline = layout.outline.inflated(1e-9);
        let clear = sheet.drawing_border.inflated(-line_mm / 2.0 - PADDING_MM + 1e-9);
        for (i, (cell, placed)) in block.cells.iter().zip(&layout.cells).enumerate() {
            assert!(outline.contains(placed.rect.min) && outline.contains(placed.rect.max), "{} {:?}", cell.name, placed.rect);
            assert!(layout.cells.iter().take(i).all(|other| !overlap(other.rect, placed.rect)), "{}", cell.name);
            let inside = placed.rect.inflated(1e-9);
            assert!(inside.contains(placed.value_box.min) && inside.contains(placed.value_box.max), "{}", cell.name);
            let room = placed.value_box.size();
            assert!(room.x > 0.0 && room.y > 0.0 && clear.contains(placed.value_box.min) && clear.contains(placed.value_box.max), "{}", cell.name);
            if let Some(at) = placed.label_at {
                assert!(inside.contains(at), "{}", cell.name);
            }
            if let CellKind::Text(text) = &cell.kind {
                let (_, warning) = place_lines(placed.value_box, 1, text.cap_mm, text.cap_mm * 1.6, text.align, false).unwrap();
                assert_eq!(warning, None, "{}", cell.name);
            }
        }
    }
    // Fewer lay out than accepted: about two in five meet a 5 or 10 mm border line, which leaves most blocks no room.
    assert!(accepted > 400 && laid_out > 200, "{accepted} accepted, {laid_out} laid out");
}

/// The built-in block's twelve cells, by name: no type, status or scale cell. The legal owner is the
/// drawing's `organization`, and the general tolerances are text to change.
#[test]
fn the_built_in_blocks_cells() {
    let block = default_block();
    let names: Vec<&str> = block.cells.iter().map(|c| c.name.as_str()).collect();
    let expected = [
        "title",
        "identification_number",
        "created_by",
        "approved_by",
        "date_of_issue",
        "sheet",
        "material",
        "finish",
        "mass",
        "legal_owner",
        "general_tolerances",
        "projection",
    ];
    assert_eq!(names, expected);
    let content = |name: &str| text_of(block.cells.iter().find(|c| c.name == name).unwrap()).content.clone();
    assert_eq!(content("legal_owner"), Content(vec![field(Source::Drawing, "organization")]));
    assert_eq!(content("general_tolerances"), Content(vec![Part::Text("ISO 2768-m".into())]));
}
