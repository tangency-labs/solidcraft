use serde_json::{Value, json};
use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_engine::geom::Loop2Geom;
use solidcraft_engine::geom::paper::{Dashes, Group, Item, ObjectKind, Scene};

use super::*;
use crate::scenario::Harness;

/// A 100 × 80 × 20 mm plate (W23's fixture without its bores) on ISO A3 at 1:2: its front view and
/// the view right of it, the front's top edge dimensioned, in the Drawing workspace, with every view
/// drawn. Returns the harness and the front view's id.
fn plate_sheet() -> (Harness, String) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.app.run("solid.box", json!({"length": 100, "width": 80, "height": 20, "body_name": "Plate"})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let front = h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [140, 180]})).unwrap();
    let front = front["view"].as_str().unwrap().to_owned();
    h.app.run("drawing.view.projected", json!({"parent": front, "side": "right"})).unwrap();
    h.app.run("drawing.dimension.add", json!({"view": front, "kind": {"length": {"edge": [50, 0, 20]}}})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    (h, front)
}

#[test]
fn note_tool_places_text_on_the_active_sheet() {
    let (mut h, _) = plate_sheet();
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.note"));
    h.frames(4);
    let field = crate::scenario::rect_of("drawing:note:text").unwrap().center();
    h.step(&json!({"click": [field.x, field.y]})).unwrap();
    h.step(&json!({"text": "Deburr all edges"})).unwrap();
    let paper = crate::scenario::rect_of("sheet:paper").unwrap().center();
    h.step(&json!({"click": [paper.x, paper.y]})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let note = &drawing.notes[0];
    assert_eq!(note.content.0, vec![solidcraft_engine::geom::cell::Part::Text("Deburr all edges".to_owned())]);
}

/// A right-click on the sheet opens the menu a sketch's right-click opens: it selects what is under
/// the pointer first, offers Edit and Delete for it, then the tools, then Undo and Redo. On empty
/// paper the selection stays, and a right-click with a tool in hand puts the tool down.
#[test]
fn right_click_opens_the_shared_menu_on_what_is_under_the_pointer() {
    use solidcraft_engine::Sel;
    let (mut h, _) = plate_sheet();
    let dimension = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let read = h.app.run("drawing.dimension.inspect", json!({"dimension": solidcraft_engine::doc::ids::encode(dimension)})).unwrap();
    let corners: Vec<[f64; 2]> = serde_json::from_value(read["layout"]["text_boxes"][0].clone()).unwrap();
    let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
    let screen = |x: f64, y: f64| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let labels = |h: &Harness| -> Vec<String> {
        crate::context_menu::describe(&h.app)["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["label"].as_str().map(str::to_owned))
            .collect()
    };
    let picked = |h: &Harness| -> Vec<u64> {
        h.app.session.selection.iter().filter_map(|s| if let Sel::Drawing { object, .. } = s { Some(*object) } else { None }).collect()
    };

    h.step(&json!({"click": screen(x, y), "button": "right"})).unwrap();
    h.frames(3);
    assert_eq!(picked(&h), [dimension], "the right-click selects the dimension under it");
    let rows = labels(&h);
    assert_eq!(&rows[..2], ["Edit Text", "Delete"], "{rows:?}");
    for row in ["Base View", "Dimension", "Note", "Undo", "Redo"] {
        assert!(rows.iter().any(|label| label == row), "{row} is offered: {rows:?}");
    }
    let undo = h.app.session.undo.len();
    crate::context_menu::pick(&mut h.app, "Delete").unwrap();
    h.frames(4);
    assert!(h.app.session.drawing("drawing.inspect").unwrap().dimension(dimension).is_none(), "the menu's Delete is the Delete key's");
    assert_eq!(h.app.session.undo.len(), undo + 1, "in one undo step");

    // Empty paper: no Edit or Delete, the tools and Undo; Undo brings the dimension back.
    h.step(&json!({"click": screen(300.0, 60.0), "button": "right"})).unwrap();
    h.frames(3);
    let rows = labels(&h);
    assert_eq!(rows.first().map(String::as_str), Some("Base View"), "{rows:?}");
    crate::context_menu::pick(&mut h.app, "Undo").unwrap();
    h.settle();
    assert!(h.app.session.drawing("drawing.inspect").unwrap().dimension(dimension).is_some());

    // A view's menu edits it; the selection of several things keeps to Delete.
    h.step(&json!({"click": screen(140.0, 180.0), "button": "right"})).unwrap();
    h.frames(3);
    let rows = labels(&h);
    assert_eq!(&rows[..3], ["Edit View", "Projected View", "Delete"], "{rows:?}");
    crate::context_menu::pick(&mut h.app, "Edit View").unwrap();
    h.frames(3);
    assert!(crate::drawing_workspace::inspector_visible(&h.app), "Edit opens the view's options, as a double-click does");
    crate::drawing_workspace::close_panel(&mut h.app);

    // The menu's tools start as their ribbon buttons do. With a tool in hand a right-click puts
    // it down, as it finishes a sketch tool.
    h.step(&json!({"click": screen(300.0, 60.0), "button": "right"})).unwrap();
    h.frames(3);
    crate::context_menu::pick(&mut h.app, "Note").unwrap();
    h.frames(3);
    assert!(crate::drawing_workspace::placing_note(&h.app));
    h.step(&json!({"click": screen(300.0, 60.0), "button": "right"})).unwrap();
    h.frames(3);
    assert!(!crate::drawing_workspace::placing_note(&h.app));
    assert_eq!(crate::context_menu::describe(&h.app)["open"], false, "and opens no menu");

    // `ui.menu` at a point opens the same menu on what is there.
    h.call("ui.menu", json!({"x": screen(140.0, 180.0)[0], "y": screen(140.0, 180.0)[1]}));
    h.frames(4);
    assert_eq!(labels(&h).first().map(String::as_str), Some("Edit View"));
    h.call("ui.menu", json!({"close": true}));
}

/// A double-click on a dimension or a note edits its text where it is, in the box a sketch
/// dimension's value is typed in: Enter changes it in one undo step, Esc leaves it, and the
/// number typed back (or nothing) shows the measured number again.
#[test]
fn double_click_edits_a_dimensions_or_a_notes_text_in_place() {
    let (mut h, _) = plate_sheet();
    let encode = solidcraft_engine::doc::ids::encode;
    let dimension = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let note = h.app.run("drawing.note.add", json!({"anchor": {"sheet": "Sheet 1"}, "at": [300, 230], "content": "DEBURR"})).unwrap();
    let note = solidcraft_engine::doc::ids::decode(note["note"].as_str().unwrap()).unwrap();
    settle(&mut h);
    let screen = |x: f64, y: f64| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let read = |h: &mut Harness| h.app.run("drawing.dimension.inspect", json!({"dimension": encode(dimension)})).unwrap();
    let text_at = |h: &mut Harness| {
        let corners: Vec<[f64; 2]> = serde_json::from_value(read(h)["layout"]["text_boxes"][0].clone()).unwrap();
        let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
        screen(x, y)
    };
    let measured = read(&mut h)["text"].as_str().unwrap().to_owned();
    let type_over = |h: &mut Harness, at: [f32; 2], text: &str, end: &str| {
        h.step(&json!({"click": at, "double": true})).unwrap();
        h.frames(4);
        assert!(crate::dim_view::editing(), "the double-click opens the box");
        assert!(!crate::drawing_workspace::inspector_visible(&h.app), "not the side panel");
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        if text.is_empty() {
            h.step(&json!({"key": "Delete"})).unwrap();
        } else {
            h.step(&json!({"text": text})).unwrap();
        }
        h.step(&json!({"key": end})).unwrap();
        h.frames(4);
        assert!(!crate::dim_view::editing(), "{end} closes the box");
    };

    let undo = h.app.session.undo.len();
    let at = text_at(&mut h);
    type_over(&mut h, at, "100 TYP", "Enter");
    assert_eq!(read(&mut h)["text"], "100 TYP");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    let at = text_at(&mut h);
    type_over(&mut h, at, "nothing", "Escape");
    assert_eq!(read(&mut h)["text"], "100 TYP", "Esc leaves the text");
    // Another text over the first is one step back to the first; the measured number typed over
    // a text shows the number again, measured, not kept as a text of its own.
    let undo = h.app.session.undo.len();
    let at = text_at(&mut h);
    type_over(&mut h, at, "SEE NOTE", "Enter");
    assert_eq!(read(&mut h)["text"], "SEE NOTE");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    h.settle();
    assert_eq!(read(&mut h)["text"], "100 TYP", "undone in one step, to the text before");
    let at = text_at(&mut h);
    type_over(&mut h, at, &measured, "Enter");
    assert!(read(&mut h)["record"]["override_text"].is_null(), "the number typed over a text is measured again");
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    h.settle();
    assert_eq!(read(&mut h)["text"], "100 TYP");
    let at = text_at(&mut h);
    type_over(&mut h, at, "", "Enter");
    assert_eq!(read(&mut h)["text"], measured.as_str(), "an empty text shows the measured number again");
    assert!(read(&mut h)["record"]["override_text"].is_null());
    let undo = h.app.session.undo.len();
    let at = text_at(&mut h);
    type_over(&mut h, at, &measured, "Enter");
    assert!(read(&mut h)["record"]["override_text"].is_null(), "the number typed back stays measured");
    assert_eq!(h.app.session.undo.len(), undo, "and is no step");

    // A note: its text box is to the right of and below where it is placed.
    let bounds = {
        let sheet = crate::drawing_sheet_page::active_sheet(&h.app).unwrap();
        let view = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
        view.objects.iter().find(|object| object.object == note).unwrap().bounds_mm
    };
    let at = screen((bounds.min.x + bounds.max.x) / 2.0, (bounds.min.y + bounds.max.y) / 2.0);
    type_over(&mut h, at, "DEBURR ALL EDGES", "Enter");
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    assert_eq!(drawing.notes.by_id(note).unwrap().content.0, vec![solidcraft_engine::geom::cell::Part::Text("DEBURR ALL EDGES".to_owned())]);

    // A view has no one text: its options open, as before.
    h.step(&json!({"click": screen(140.0, 180.0), "double": true})).unwrap();
    h.frames(4);
    assert!(!crate::dim_view::editing() && crate::drawing_workspace::inspector_visible(&h.app));
}

/// The one thing selected shows its options in the right panel, where a tool shows its own, and
/// one Esc clears the selection and the panel with it, as in a sketch. A dimension's options apply:
/// its offset changes in one undo step.
#[test]
fn the_selected_object_shows_its_options_and_a_dimensions_apply() {
    let (mut h, _) = plate_sheet();
    let encode = solidcraft_engine::doc::ids::encode;
    let dimension = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let screen = |x: f64, y: f64| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let read = |h: &mut Harness| h.app.run("drawing.dimension.inspect", json!({"dimension": encode(dimension)})).unwrap();
    let corners: Vec<[f64; 2]> = serde_json::from_value(read(&mut h)["layout"]["text_boxes"][0].clone()).unwrap();
    let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
    assert!(!crate::drawing_workspace::inspector_visible(&h.app));

    h.step(&json!({"click": screen(x, y)})).unwrap();
    h.frames(4);
    assert!(crate::drawing_workspace::inspector_visible(&h.app), "the selected dimension's options show");
    assert_eq!(crate::drawing_workspace::inspector_title(&h.app), "Dimension");
    let offset = read(&mut h)["record"]["offset_mm"].as_f64().unwrap();
    let field = h.widget(&format!("{offset} mm")).expect("the Offset field shows the dimension's offset");
    let undo = h.app.session.undo.len();
    h.step(&json!({"click": field})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": format!("{}", offset * 2.0)})).unwrap();
    let apply = h.widget("Apply").unwrap();
    h.step(&json!({"click": apply})).unwrap();
    h.frames(4);
    assert_eq!(read(&mut h)["record"]["offset_mm"].as_f64(), Some(offset * 2.0), "Apply moves the dimension line");
    assert_eq!(h.app.session.undo.len(), undo + 1, "in one undo step");
    assert!(crate::drawing_workspace::inspector_visible(&h.app), "Apply keeps the panel");

    // One Esc, as in a sketch: the selection goes, and its options with it.
    h.step(&json!({"click": screen(300.0, 250.0)})).unwrap();
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(4);
    assert!(h.app.session.selection.is_empty());
    assert!(!crate::drawing_workspace::inspector_visible(&h.app));

    // A view's options likewise; two things selected have no one panel.
    h.step(&json!({"click": screen(140.0, 180.0)})).unwrap();
    h.frames(4);
    assert_eq!(crate::drawing_workspace::inspector_title(&h.app), "Edit View");
    let corners: Vec<[f64; 2]> = serde_json::from_value(read(&mut h)["layout"]["text_boxes"][0].clone()).unwrap();
    let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
    h.step(&json!({"click": screen(x, y), "shift": true})).unwrap();
    h.frames(4);
    assert_eq!(h.app.session.selection.len(), 2);
    assert!(!crate::drawing_workspace::inspector_visible(&h.app));

    // A tool in hand keeps its own panel whatever is selected.
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.note"));
    h.frames(2);
    assert_eq!(crate::drawing_workspace::inspector_title(&h.app), "Note");
    assert!(crate::drawing_workspace::placing_note(&h.app));
}

/// A double-click on a text cell of the title block edits what it shows, in the box a dimension's
/// text is typed in: Enter keeps the text over the cell's own value in one undo step, and nothing
/// typed shows the cell's own value again. The projection symbol's cell has no text to edit.
#[test]
fn double_click_edits_a_title_block_cells_text_in_place() {
    use solidcraft_engine::geom::title_block::CellKind;
    let (mut h, _) = plate_sheet();
    let centre = |h: &Harness, text: bool| {
        let drawing = h.app.session.drawing("drawing.inspect").unwrap();
        let record = &drawing.sheets[0];
        let (block, placed) = (record.title_block.as_ref().unwrap(), record.title_block_layout().unwrap().unwrap());
        let at =
            block.cells.iter().position(|cell| if text { cell.name == "title" } else { matches!(cell.kind, CellKind::ProjectionSymbol) }).unwrap();
        let rect = placed.cells[at].rect;
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        let (x, y) = ((rect.min.x + rect.max.x) / 2.0, (rect.min.y + rect.max.y) / 2.0);
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let typed = |h: &Harness| h.app.session.drawing("drawing.inspect").unwrap().sheets[0].title_block_overrides.get("title").cloned();
    let type_over = |h: &mut Harness, text: &str| {
        h.step(&json!({"click": centre(h, true), "double": true})).unwrap();
        h.frames(4);
        assert!(crate::dim_view::editing(), "the double-click opens the box on the Title cell");
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        if text.is_empty() {
            h.step(&json!({"key": "Delete"})).unwrap();
        } else {
            h.step(&json!({"text": text})).unwrap();
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(4);
        assert!(!crate::dim_view::editing());
    };
    assert_eq!(typed(&h), None);
    let undo = h.app.session.undo.len();
    type_over(&mut h, "MOUNTING PLATE");
    assert_eq!(typed(&h).as_deref(), Some("MOUNTING PLATE"));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    type_over(&mut h, "MOUNTING PLATE");
    assert_eq!(h.app.session.undo.len(), undo + 1, "the same text again is no step");
    type_over(&mut h, "");
    assert_eq!(typed(&h), None, "nothing typed shows the cell's own value again");

    h.step(&json!({"click": centre(&h, false), "double": true})).unwrap();
    h.frames(4);
    assert!(!crate::dim_view::editing(), "the projection symbol has no text");
}

#[test]
fn dimension_tool_places_a_view_edge_length() {
    let (mut h, front) = plate_sheet();
    let existing = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    let screen = |x: f32, y: f32| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x / 420.0, paper.bottom() - paper.height() * y / 297.0]
    };
    h.step(&json!({"click": screen(140.0, 175.0)})).unwrap();
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), 1);
    h.frames(4);
    h.step(&json!({"click": screen(140.0, 163.0)})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    assert_eq!(drawing.dimensions.len(), 2);
    let placed = drawing.dimensions.iter().find(|dimension| dimension.id != existing).unwrap();
    assert_eq!(solidcraft_engine::doc::ids::encode(placed.view), front);
    assert!((placed.offset_mm.abs() - 12.0).abs() < 0.75, "offset was {} mm", placed.offset_mm);
}

/// The plate's sheet with a hole 12 across through the plate's middle, seen face on in the front
/// view: on paper (1:2, centred at 140, 180) the plate's bottom edge is y = 175, its top y = 185, its
/// left side x = 115 and the hole a circle of radius 3 about (140, 180). The Dimension tool is on.
fn holed_plate_dimensioning() -> (Harness, String) {
    let (mut h, front) = plate_sheet();
    crate::drawing_workspace::enter(&mut h.app, false).unwrap();
    let cut = json!({"radius": 6, "height": 100, "base": [50, -10, 10], "axis": "Y", "operation": "cut", "targets": ["Plate"], "name": "Hole"});
    h.app.run("solid.cylinder", cut).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    (h, front)
}

/// A click at paper millimetres (`x`, `y`) of the A3 sheet, a frame drawn after it.
fn click_paper(h: &mut Harness, x: f32, y: f32) {
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let at = [paper.left() + paper.width() * x / 420.0, paper.bottom() - paper.height() * y / 297.0];
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
}

/// The dimension the tool added last, as inspect reads it.
fn added_dimension(h: &mut Harness, before: &[u64]) -> Value {
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    assert_eq!(drawing.dimensions.len(), before.len() + 1, "one dimension was added");
    let id = drawing.dimensions.iter().map(|d| d.id).find(|id| !before.contains(id)).unwrap();
    h.app.run("drawing.dimension.inspect", json!({"dimension": solidcraft_engine::doc::ids::encode(id)})).unwrap()
}

/// Two edges picked one after the other dimension the distance between them, as in a sketch: the
/// plate's bottom and top edges, parallel, read its 20 mm height, measured square to them
/// (vertical), with the line through the third click.
#[test]
fn dimension_tool_dimensions_between_two_parallel_edges() {
    let (mut h, _) = holed_plate_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    click_paper(&mut h, 125.0, 175.0);
    click_paper(&mut h, 125.0, 185.0);
    use crate::drawing_workspace::dimension::{Shape, picked_shapes};
    assert!(matches!(picked_shapes(&h.app)[..], [Shape::Straight(_), Shape::Straight(_)]), "{:?}", picked_shapes(&h.app));
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), before.len(), "two picks place nothing yet");
    click_paper(&mut h, 100.0, 180.0);
    let read = added_dimension(&mut h, &before);
    assert!((number(&read["value"]) - 20.0).abs() < 1e-6, "{read}");
    assert_eq!(read["record"]["kind"]["linear"]["direction"], "vertical", "{read}");
    assert!(picked_shapes(&h.app).is_empty(), "the tool starts again, as the sketch's does");
}

/// Two edges that aren't parallel dimension the angle between them: the plate's bottom edge and its
/// left side, 90°.
#[test]
fn dimension_tool_dimensions_the_angle_between_two_edges() {
    let (mut h, _) = holed_plate_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    click_paper(&mut h, 125.0, 175.0);
    click_paper(&mut h, 115.0, 179.0);
    click_paper(&mut h, 122.0, 181.0);
    let read = added_dimension(&mut h, &before);
    assert!(read["record"]["kind"]["angle"].is_object(), "{read}");
    assert!((number(&read["value"]) - 90.0).abs() < 1e-6, "{read}");
}

/// One round edge dimensions its size: the hole's rim, a whole circle, reads Ø12, its text where
/// the click placed it.
#[test]
fn dimension_tool_dimensions_a_holes_diameter() {
    let (mut h, _) = holed_plate_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    click_paper(&mut h, 143.0, 180.0);
    use crate::drawing_workspace::dimension::{Shape, picked_shapes};
    assert!(matches!(picked_shapes(&h.app)[..], [Shape::Round(_)]), "{:?}", picked_shapes(&h.app));
    click_paper(&mut h, 150.0, 192.0);
    let read = added_dimension(&mut h, &before);
    assert!(read["record"]["kind"]["radial"].is_object(), "{read}");
    assert_eq!(read["text"], "Ø12.00", "{read}");
}

/// Esc drops the tool's picks first and keeps the tool, as in a sketch; the next Esc leaves it.
#[test]
fn dimension_tool_escape_drops_picks_then_the_tool() {
    let (mut h, _) = holed_plate_dimensioning();
    click_paper(&mut h, 125.0, 175.0);
    assert_eq!(crate::drawing_workspace::dimension::picked_shapes(&h.app).len(), 1);
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(crate::drawing_workspace::dimension::picked_shapes(&h.app).is_empty());
    assert!(crate::drawing_workspace::placing_dimension(&h.app), "the tool stays on");
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(!crate::drawing_workspace::placing_dimension(&h.app));
}

/// Frames until no view or dimension is waiting to be drawn.
fn settle(h: &mut Harness) {
    for _ in 0..5_000 {
        h.frame();
        if page(h)["working"] == false {
            h.frames(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the sheet never finished drawing: {}", page(h));
}

fn page(h: &mut Harness) -> Value {
    h.call("ui.inspect", json!({}))["result"]["sheet_page"].clone()
}

/// What the last frame showed, without running another.
fn inspect_now(h: &Harness) -> Value {
    super::inspect(&h.app)
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// 100 % is (96 / 25.4) × calibration points per paper millimetre over the UI zoom, so a
/// millimetre is the same number of physical pixels at every UI scale; a calibration out of range
/// is not used.
#[test]
fn true_size_is_one_millimetre_at_any_ui_scale() {
    for native in [1.0f32, 1.25, 2.0] {
        for calibration in [0.5f32, 1.0, 1.25, 2.0] {
            let pixels: Vec<f64> = [0.75f32, 1.0, 1.5, 2.5].iter().map(|zoom| true_size(calibration, *zoom) * f64::from(native * zoom)).collect();
            let want = 96.0 / 25.4 * f64::from(native * calibration);
            assert!(pixels.iter().all(|p| (p - want).abs() < 1e-9), "{native} {calibration}: {pixels:?}");
        }
    }
    for hostile in [0.0f32, -1.0, 2.5, f32::NAN, f32::INFINITY] {
        assert_eq!(true_size(hostile, 1.0), true_size(1.0, 1.0), "calibration {hostile}");
    }
    for hostile in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(true_size(1.0, hostile), true_size(1.0, 1.0), "UI zoom {hostile}");
    }
    // On the page: at 100 % and UI scale 100 %, a 50 mm edge is 188.98 points (W23-4).
    let (mut h, _) = plate_sheet();
    assert_eq!(h.call("ui.view", json!({"view": "actual_size"}))["ok"], true);
    h.frames(2);
    let p = page(&mut h);
    assert!((number(&p["zoom"]) - 1.0).abs() < 1e-9, "{p}");
    assert!((50.0 * number(&p["points_per_mm"]) - 188.976).abs() < 1e-3, "{p}");
    let visible = p["visible_mm"].as_array().unwrap().iter().map(number).collect::<Vec<_>>();
    let canvas = h.app.viewport.rect.unwrap();
    assert!(((visible[2] - visible[0]) * number(&p["points_per_mm"]) - f64::from(canvas.width())).abs() < 1e-3, "{p}");
    // A calibration of 1.25 makes 100 % a quarter larger.
    h.app.preferences.drawing_calibration = 1.25;
    h.step(&json!({"click": {"handle": "nav:slot:sheet_zoom"}})).unwrap();
    h.step(&json!({"click": {"handle": "sheet:actual_size"}})).unwrap();
    h.frames(2);
    let p = page(&mut h);
    assert!((number(&p["points_per_mm"]) - 1.25 * 96.0 / 25.4).abs() < 1e-9 && (number(&p["zoom"]) - 1.0).abs() < 1e-9, "{p}");
}

/// #19070: Fit Sheet frames the sheet in the part of the window the ribbon, the Browser and the
/// History leave free, so no edge of the paper is under them, in a small window too.
#[test]
fn fit_sheet_keeps_the_paper_clear_of_the_ribbon_browser_and_history() {
    let (mut h, _) = plate_sheet();
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    h.frames(6);
    h.app.run("view.home", json!({})).unwrap();
    h.frames(4);
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let canvas = h.app.viewport.rect.unwrap();
    let mut covers: Vec<(&str, Rect)> = h.app.shell.cards.iter().map(|r| ("a ribbon card", *r)).collect();
    covers.extend(h.app.shell.browser.map(|r| ("the Browser", r)));
    covers.extend(h.app.shell.history.map(|r| ("the History bar", r)));
    assert!(!covers.is_empty(), "the shell reports what covers the canvas");
    for (what, rect) in covers.into_iter().filter(|(_, r)| r.intersects(canvas)) {
        assert!(!paper.intersects(rect), "the paper {paper:?} is under {what} {rect:?}");
    }
}

/// A short Browser (folded, or on a small window) is wider than tall, yet lies against the left
/// edge: the paper is framed to its right, not below it.
#[test]
fn fit_sheet_frames_beside_a_short_browser_not_under_it() {
    let (mut h, _) = plate_sheet();
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    h.frames(6);
    let canvas = h.app.viewport.rect.unwrap();
    let short = Rect::from_min_size(canvas.min + vec2(8.0, 180.0), vec2(260.0, 60.0));
    h.app.shell.browser = Some(short);
    h.app.ui.show_browser = true;
    h.app.run("view.home", json!({})).unwrap();
    h.frames(4);
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    assert!(!paper.intersects(short), "the paper {paper:?} is under the short Browser {short:?}");
    assert!(paper.min.x >= short.right() - 1.0, "the paper {paper:?} is framed beside the Browser {short:?}");
}

/// Fit Sheet shows the whole sheet, and comes back after any zoom; typed and wheeled zooms stay
/// within 5 % to 3200 %, and a zoom that isn't a number changes nothing.
#[test]
fn fit_sheet_shows_the_whole_sheet_and_zoom_stays_in_range() {
    let (mut h, _) = plate_sheet();
    let whole = |p: &Value| {
        let v = p["visible_mm"].as_array().unwrap().iter().map(number).collect::<Vec<_>>();
        v[0] <= 0.0 && v[1] <= 0.0 && v[2] >= 420.0 && v[3] >= 297.0
    };
    let p = page(&mut h);
    assert_eq!(p["size_mm"], json!([420.0, 297.0]));
    assert!(whole(&p), "the first frame fits the sheet: {p}");
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let canvas = h.app.viewport.rect.unwrap();
    assert!(canvas.contains_rect(paper), "{paper:?} in {canvas:?}");
    // Fitted above the navigation bar, as the design's layout board has it.
    let bar = h.app.viewport.layout.as_ref().unwrap().navigation;
    assert!(bar.is_positive() && !paper.intersects(bar), "{paper:?} clear of the navigation bar {bar:?}");
    // A fitted sheet is fitted again when the window changes size, until something moves it.
    h.call("ui.resize", json!({"width": 900, "height": 600}));
    h.frames(4);
    assert!(whole(&page(&mut h)), "refitted in a smaller window: {}", page(&mut h));
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.frames(4);
    let ctx = h.ctx.clone();
    set_zoom(&mut h.app, &ctx, 1.5);
    h.frames(2);
    h.call("ui.resize", json!({"width": 1200, "height": 800}));
    h.frames(4);
    assert!((number(&page(&mut h)["zoom"]) - 1.5).abs() < 1e-9, "a chosen zoom stays through a resize");
    for (zoom, want) in [(1000.0, 32.0), (0.001, 0.05), (2.0, 2.0)] {
        let ctx = h.ctx.clone();
        set_zoom(&mut h.app, &ctx, zoom);
        h.frames(2);
        assert!((number(&page(&mut h)["zoom"]) - want).abs() < 1e-9, "{zoom}");
    }
    let ctx = h.ctx.clone();
    set_zoom(&mut h.app, &ctx, f64::NAN);
    set_zoom(&mut h.app, &ctx, f64::INFINITY);
    h.frames(2);
    assert!((number(&page(&mut h)["zoom"]) - 2.0).abs() < 1e-9);
    assert!(!whole(&page(&mut h)), "at 200 % the sheet is larger than the canvas");
    // The wheel zooms in no further than 3200 %.
    let centre = h.app.viewport.rect.unwrap().center();
    for _ in 0..80 {
        h.app.synthetic.push(egui::Event::PointerMoved(centre));
        h.app.synthetic.push(egui::Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
    }
    // egui's smoothed scrolling runs on for some frames after the last wheel event.
    h.app.synthetic.push(egui::Event::PointerGone);
    h.frames(120);
    let zoom = number(&page(&mut h)["zoom"]);
    assert!(zoom > 2.0 && zoom <= 32.0 + 1e-9, "{zoom}");
    // Fit (F6 runs view.fit), Home and the zoom menu's Fit sheet all frame it again.
    for fit in [json!({"command": "view.fit"}), json!({"command": "view.home"})] {
        set_zoom(&mut h.app, &ctx, 4.0);
        h.frames(2);
        h.app.run(fit["command"].as_str().unwrap(), json!({})).unwrap();
        h.frames(2);
        assert!(whole(&page(&mut h)), "{fit}");
    }
    set_zoom(&mut h.app, &ctx, 4.0);
    h.frames(2);
    h.step(&json!({"click": {"handle": "nav:slot:sheet_zoom"}})).unwrap();
    h.step(&json!({"click": {"handle": "sheet:fit"}})).unwrap();
    h.frames(2);
    assert!(whole(&page(&mut h)));
    assert!(h.call("ui.view", json!({"view": "front"}))["ok"] == false, "a sheet has no 3D views");
}

/// The navigation bar shows whole percent, and tenths below 10 %; typing a shown value back keeps it.
#[test]
fn the_zoom_readout_is_rounded() {
    for (zoom, shown) in [(0.72996, 73.0), (1.0, 100.0), (0.0549, 5.5), (0.05, 5.0), (0.0996, 10.0), (31.99, 3199.0)] {
        assert_eq!(shown_percent(zoom), shown, "{zoom}");
    }
}

/// The pan tool drags the sheet with the pointer on the page's own camera: the Design camera, the
/// design and its undo steps stay as they were.
#[test]
fn pan_moves_the_sheet_and_leaves_the_design_camera() {
    let (mut h, _) = plate_sheet();
    let (cam, doc, undo) = (h.app.cam, h.app.session.doc.to_json(), h.app.session.undo.len());
    h.app.run("view.tool", json!({"tool": "pan"})).unwrap();
    let before = crate::scenario::rect_of("sheet:paper").unwrap();
    // From an empty corner of the paper, not the canvas's middle: where the fitted sheet puts its
    // views depends on the free area around it.
    let from = before.min + vec2(15.0, 15.0);
    h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": from.x + 120.0, "y1": from.y + 40.0, "steps": 8}));
    h.frames(2);
    let after = crate::scenario::rect_of("sheet:paper").unwrap();
    let moved = after.min - before.min;
    assert!((moved.x - 120.0).abs() < 2.0 && (moved.y - 40.0).abs() < 2.0, "{moved:?}");
    assert!((after.width() - before.width()).abs() < 1e-3, "panning doesn't zoom");
    assert_eq!(h.app.cam, cam);
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
}

/// Each sheet colour is drawn in its tokens, standing off the backdrop, and is screen only: the
/// design, its undo steps and the exported SVG are the same bytes under every one.
#[test]
fn sheet_colour_is_screen_only() {
    let (mut h, _) = plate_sheet();
    let (doc, undo) = (h.app.session.doc.to_json(), h.app.session.undo.len());
    let svg = |h: &mut Harness| h.app.run("drawing.export", json!({"sheet": "Sheet 1"})).unwrap()["svg"].as_str().unwrap().to_owned();
    let first = svg(&mut h);
    let t = Tokens::get();
    let mut seen = Vec::new();
    for colour in SheetColour::ALL {
        h.app.preferences.drawing_sheet_colour = colour.id().into();
        h.frames(2);
        let p = page(&mut h);
        let (paper, ink) = colour.paper(&t, solidcraft_theme::is_dark());
        let (stage, paper) = stage_and_paper(paper, t.viewport_top);
        assert_eq!((&p["stage"], &p["paper"]), (&json!(hex(stage)), &json!(hex(paper))), "{colour:?}");
        assert_eq!(p["ink"], json!(hex(ink)), "{colour:?}");
        seen.push(p["paper"].clone());
        assert_eq!(svg(&mut h), first, "{colour:?}");
    }
    assert_ne!(seen[1], seen[2], "White and Blueprint differ");
    assert_ne!(seen[2], seen[3], "Blueprint and Prussian Blue differ");
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
    let bad = h.call("ui.prefs", json!({"drawing_sheet_colour": "blue"}));
    assert_eq!(bad["ok"], false, "{bad}");
    let bad = h.call("ui.prefs", json!({"drawing_calibration": 3.0}));
    assert_eq!(bad["ok"], false, "{bad}");
}

/// The backdrop has no effect on the design, undo steps or exported SVG.
#[test]
fn sheet_backdrop_is_a_screen_only_preference() {
    let (mut h, _) = plate_sheet();
    assert_eq!(h.app.preferences.drawing_sheet_backdrop, "none");
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let svg = |h: &mut Harness| h.app.run("drawing.export", json!({"sheet": "Sheet 1"})).unwrap()["svg"].as_str().unwrap().to_owned();
    let first = svg(&mut h);
    for (_, id, _) in SheetBackdrop::ALL {
        assert_eq!(h.call("ui.prefs", json!({"drawing_sheet_backdrop": id}))["ok"], true);
        h.frames(2);
        assert_eq!(h.app.preferences.drawing_sheet_backdrop, id);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(svg(&mut h), first);
    }
    let bad = h.call("ui.prefs", json!({"drawing_sheet_backdrop": "invalid"}));
    assert_eq!(bad["ok"], false);
    assert_eq!(h.app.preferences.drawing_sheet_backdrop, "none");
}

/// The sheet as SVG and as a 150 dpi PNG, each written to a file by `drawing.export` sent over the
/// control channel (which carries the window's context, as an agent's export does), read back:
/// `tag` names the files.
fn exported(h: &mut Harness, tag: &str) -> [Vec<u8>; 2] {
    ["svg", "png"].map(|format| {
        let path = h.dir.join(format!("{tag}.{format}"));
        let dpi = (format == "png").then_some(150);
        let mut p = json!({"sheet": "Sheet 1", "format": format, "path": path.to_string_lossy()});
        if let Some(dpi) = dpi {
            p["dpi"] = json!(dpi);
        }
        let reply = h.call("engine.execute", json!({"command": "drawing.export", "params": p}));
        assert_eq!(reply["ok"], true, "{reply}");
        std::fs::read(&path).unwrap()
    })
}

/// A drawing's exports don't depend on the window (#4894 W23-3; Cam, 2 Nov): the sheet is built in
/// millimetres at a fixed dpi, so the window's scale and size never reach the file. Cam's two
/// screens: the same session exports on a 4K screen (2 pixels a point, the window filling it), is
/// dragged to a 1080p screen (1 pixel a point, a smaller window) and draws a few frames, then
/// exports again; and at the harness's own scale in the plate sheet's window. The window's context
/// then has 2 and 1 pixels a point. The SVG and the PNG are the same bytes every time.
#[test]
fn a_drawing_exports_the_same_on_a_4k_and_a_1080p_screen() {
    let (mut h, _) = plate_sheet();
    let here = exported(&mut h, "here");
    // The 4K screen at 200 %: 3840 x 2160 pixels are 1920 x 1080 points.
    h.screen_scale = Some(2.0);
    h.call("ui.resize", json!({"width": 1920, "height": 1080}));
    h.frames(3);
    assert_eq!(h.ctx.pixels_per_point(), 2.0);
    let on_4k = exported(&mut h, "4k");
    // Dragged to the 1080p screen at 100 %, in a window smaller than the screen.
    h.screen_scale = Some(1.0);
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    h.frames(3);
    assert_eq!(h.ctx.pixels_per_point(), 1.0);
    let on_1080p = exported(&mut h, "1080p");
    for (screen, files) in [("4K", &on_4k), ("1080p", &on_1080p)] {
        assert!(files[0] == here[0], "the SVG on the {screen} screen differs");
        assert!(files[1] == here[1], "the PNG on the {screen} screen differs");
    }
    assert!(here[0].starts_with(b"<?xml") && here[1].starts_with(b"\x89PNG"));
}

/// Every choice, in either theme, stands off the stage behind it, and White is never pure white.
#[test]
fn every_sheet_colour_stands_off_the_backdrop() {
    crate::test_support::host_data();
    for t in [Tokens::light(), Tokens::dark()] {
        for dark in [false, true] {
            for colour in SheetColour::ALL {
                let (paper, ink) = colour.paper(&t, dark);
                let (stage, shown) = stage_and_paper(paper, t.viewport_top);
                assert!((lightness(shown) - lightness(stage)).abs() >= STAND_OFF, "{colour:?} {dark}");
                assert!((lightness(shown) - lightness(ink)).abs() > 0.5, "{colour:?} {dark}: ink reads on its paper");
            }
        }
        assert_ne!(t.paper_white, Color32::WHITE);
        // White keeps its colour in the light theme: the stage behind it goes darker instead.
        assert_eq!(stage_and_paper(t.paper_white, Tokens::light().viewport_top).1, t.paper_white);
        assert_eq!(SheetColour::Auto.paper(&t, false), SheetColour::White.paper(&t, false));
        assert_eq!(SheetColour::Auto.paper(&t, true), (t.paper_dark, t.paper_dark_ink));
    }
    // "Follows the theme" in the Blueprint theme is its blueprint blue (D24, D25).
    let blueprint = crate::theme_files::resolve(crate::theme_files::find("blueprint", &[]).unwrap());
    assert_eq!(SheetColour::Auto.paper(&blueprint, true).0, Tokens::dark().paper_blueprint);
    for colour in SheetColour::ALL {
        assert_eq!(SheetColour::parse(colour.id()), Some(colour));
    }
    assert_eq!(SheetColour::parse("Blueprint"), None);
    assert_eq!(SheetColour::parse("warm"), None);
}

fn square(at: Vec2, side: f64) -> Loop2 {
    let p = [at, at + Vec2::new(side, 0.0), at + Vec2::new(side, side), at + Vec2::new(0.0, side)];
    Loop2 { segs: (0..4).map(|i| Seg2::Line { a: p[i], b: p[(i + 1) % 4] }).collect() }
}

/// A stroke, a dashed stroke, an arc and a fill with a hole each become what they are: the dashes
/// at their lengths, the arc within the tolerance, the fill's triangles its area.
#[test]
fn marks_draw_each_item_of_the_scene() {
    let line =
        Item::Stroke { path: vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }], closed: false, width_mm: 0.5, pattern: None };
    let dashed = Item::Stroke {
        path: vec![Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(30.0, 10.0) }],
        closed: false,
        width_mm: 0.35,
        pattern: Some(Dashes { lengths_mm: vec![5.0, 5.0], phase_mm: 0.0 }),
    };
    let arc = Item::Stroke {
        path: vec![Seg2::Arc { center: Vec2::new(50.0, 50.0), radius: 20.0, start: 0.0, sweep: std::f64::consts::PI }],
        closed: false,
        width_mm: 0.5,
        pattern: None,
    };
    let fill =
        Item::Fill { region: Region2 { outer: square(Vec2::new(60.0, 0.0), 10.0), holes: vec![square(Vec2::new(62.0, 2.0), 2.0).reversed()] } };
    let scene = Scene {
        size_mm: Vec2::new(100.0, 100.0),
        groups: vec![Group { object: 1, kind: ObjectKind::View, items: vec![line, dashed, arc, fill] }],
        fonts: Vec::new(),
        clips: Vec::new(),
    };
    let marks = Marks::of(&scene, 7, 0.01);
    assert!(marks.error.is_none(), "{:?}", marks.error);
    let lines: Vec<&Vec<Vec2>> = marks.marks.iter().filter_map(|m| if let Mark::Line { points, .. } = m { Some(points) } else { None }).collect();
    assert_eq!(lines.len(), 1 + 3 + 1, "the line, three dashes, the arc");
    for dash in &lines[1..4] {
        let length: f64 = dash.windows(2).map(|w| w[0].dist(w[1])).sum();
        assert!((length - 5.0).abs() < 1e-9, "{dash:?}");
    }
    assert!(lines[4].iter().all(|p| (p.dist(Vec2::new(50.0, 50.0)) - 20.0).abs() <= 0.01 + 1e-9) && lines[4].len() > 20);
    let Some(Mark::Fill { triangles, .. }) = marks.marks.last() else { panic!("{:?}", marks.marks) };
    let area: f64 = triangles.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).abs() / 2.0).sum();
    assert!((area - 96.0).abs() < 1e-6, "{area}");
    // A scene that isn't valid draws nothing and says why, never panics.
    let mut bad = scene.clone();
    bad.size_mm = Vec2::new(f64::NAN, 1.0);
    let marks = Marks::of(&bad, 8, 0.01);
    assert!(marks.marks.is_empty() && marks.error.is_some());
}

/// The plate's sheet draws its border, views, dimension and title block text: every text run's
/// glyphs are filled, and all of it lies on the paper.
#[test]
fn the_plate_sheet_draws_its_lines_and_text() {
    let (mut h, _) = plate_sheet();
    let sheet = active_sheet(&h.app).unwrap();
    let view = solidcraft_engine::drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    let glyphs: usize = view.scene.groups.iter().flat_map(|g| &g.items).map(|i| if let Item::Text(run) = i { run.glyphs.len() } else { 0 }).sum();
    assert!(glyphs > 20, "the title block has text: {glyphs}");
    assert!(view.scene.groups.iter().any(|g| g.kind == ObjectKind::Dimension), "the dimension is drawn");
    let marks = Marks::of(&view.scene, view.generation, 0.01);
    assert!(marks.error.is_none());
    let fills = marks.marks.iter().filter(|m| matches!(m, Mark::Fill { .. })).count();
    assert!(fills >= glyphs / 2, "{fills} fills for {glyphs} glyphs (a space has none)");
    let paper = Aabb2 { min: Vec2::ZERO, max: Vec2::new(420.0, 297.0) }.inflated(1.0);
    for m in &marks.marks {
        let b = match m {
            Mark::Line { bounds, .. } | Mark::Fill { bounds, .. } => bounds,
            Mark::Picture { rect, .. } => rect,
        };
        assert!(paper.contains(b.min) && paper.contains(b.max), "{b:?}");
    }
    assert_eq!(page(&mut h)["views"].as_array().unwrap().len(), 2);
    assert!(page(&mut h)["views"].as_array().unwrap().iter().all(|v| v["status"] == "drawn" && v["words"] == json!([])));
}

/// Every textured mesh in `shapes` (a picture's pixels; text and fills use the font texture), with the
/// screen box of its vertices.
fn textured(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::Mesh, Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Mesh, Rect)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() && !mesh.vertices.is_empty() => {
                let bounds = Rect::from_points(&mesh.vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
                out.push(((**mesh).clone(), bounds));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

/// A shaded view's picture is painted on the sheet page, over the picture's place on the sheet, once
/// it is drawn: not only its dashed frame (#18413).
#[test]
fn a_shaded_views_picture_is_painted_on_the_page() {
    let (mut h, front) = plate_sheet();
    h.app.run("drawing.view.edit", json!({"view": front, "set": {"visual_style": "shaded"}})).unwrap();
    settle(&mut h);
    let sheet = active_sheet(&h.app).unwrap();
    let view = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    let picture = view
        .scene
        .groups
        .iter()
        .filter(|g| solidcraft_engine::doc::ids::encode(g.object) == front)
        .flat_map(|g| &g.items)
        .find_map(|i| if let Item::Picture(p) = i { Some(*p) } else { None })
        .expect("a shaded view draws a picture");
    let pixels = view.pictures.iter().find(|p| p.source == picture.source).expect("the engine hands the page its pixels");
    assert!(pixels.rows.is_some() && !pixels.stale, "the picture is drawn: {:?}", pixels.why);
    // The paper's screen rectangle maps millimetres to points: the picture's rectangle on screen.
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let size = view.scene.size_mm;
    let screen = |p: Vec2| pos2(paper.left() + (p.x / size.x) as f32 * paper.width(), paper.bottom() - (p.y / size.y) as f32 * paper.height());
    let expected = Rect::from_two_pos(screen(picture.rect.min), screen(picture.rect.max));
    let meshes = textured(&h.shapes);
    assert!(
        meshes.iter().any(|(_, b)| (b.min - expected.min).length() < 1.5 && (b.max - expected.max).length() < 1.5),
        "a textured mesh covers the picture's place {expected:?}: {:?}",
        meshes.iter().map(|(_, b)| *b).collect::<Vec<_>>()
    );
}

/// A picture of a `width` × `height` grid, one millimetre a pixel, with its lower-left corner at the
/// sheet's origin: red pixels, or none (`rows: None`) and `why`.
fn hand_picture(source: u64, width: u32, height: u32, rows: bool, why: Option<&str>) -> SheetPicture {
    let grid = PixelGrid { x0: 0.0, y0: 0.0, pitch: 1.0, width, height };
    let rows = rows.then(|| Arc::from([255u8, 0, 0, 255].repeat((width * height) as usize)));
    SheetPicture { source, view: 1, grid, rows, stale: false, why: why.map(str::to_owned) }
}

/// One view's group holding a picture over `rect`, on a 100 mm square sheet, cut to `clip`.
fn picture_scene(source: u64, rect: Aabb2, opacity: f64, clip: Option<Clip>) -> Scene {
    let item = Item::Picture(solidcraft_engine::geom::paper::Picture { source, rect, opacity });
    let groups = vec![Group { object: 1, kind: ObjectKind::View, items: vec![item] }];
    Scene { size_mm: Vec2::new(100.0, 100.0), groups, fonts: Vec::new(), clips: clip.map(|c| vec![(1, c)]).unwrap_or_default() }
}

/// What `marks` paints with `pictures`, at 10 points a millimetre with the paper's origin at (0, 0).
fn painted(ctx: &egui::Context, marks: &Marks, pictures: &PictureTextures) -> Vec<egui::epaint::ClippedShape> {
    let at = OnScreen { centre: Pos2::ZERO, target: Vec2::ZERO, scale: 10.0 };
    let visible = Aabb2 { min: Vec2::new(-1e3, -1e3), max: Vec2::new(1e3, 1e3) };
    ctx.run_ui(egui::RawInput::default(), |ui| marks.paint(ui.painter(), &at, visible, Color32::BLACK, 1.0, pictures)).shapes
}

/// Each text shape's words in `shapes`.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

/// A picture is uploaded once for its rows: the same rows again upload nothing, new rows (another
/// allocation, even of the same bytes) upload once more, and a picture that leaves the sheet is let go.
#[test]
fn a_picture_uploads_once_and_again_only_for_new_rows() {
    let ctx = egui::Context::default();
    let mut pictures = PictureTextures::default();
    let first = hand_picture(7, 4, 2, true, None);
    for _ in 0..3 {
        pictures.sync(&ctx, std::slice::from_ref(&first), 2048);
    }
    assert_eq!(pictures.uploads, 1, "the same rows upload once");
    let again = hand_picture(7, 4, 2, true, None);
    pictures.sync(&ctx, &[again], 2048);
    assert_eq!(pictures.uploads, 2, "new rows upload again");
    pictures.sync(&ctx, &[], 2048);
    assert!(pictures.get(7).is_none(), "a picture that left the sheet is let go");
    // Rows that aren't the grid's size upload nothing, and the picture keeps its frame.
    let mut short = hand_picture(8, 4, 2, true, None);
    short.rows = Some(Arc::from(vec![0u8; 5]));
    pictures.sync(&ctx, &[short], 2048);
    assert!(pictures.get(8).is_some_and(|u| u.tiles.is_empty()));
}

/// A picture not drawn yet shows its dashed frame and no pixels; one that can't be drawn says why,
/// in words beside its frame (D26).
#[test]
fn a_picture_not_drawn_shows_its_frame_and_one_that_failed_says_why() {
    // The catalogs, as the app installs them at start: the words are a message.
    crate::i18n::install();
    let ctx = egui::Context::default();
    let rect = Aabb2 { min: Vec2::new(10.0, 10.0), max: Vec2::new(40.0, 30.0) };
    let marks = Marks::of(&picture_scene(7, rect, 1.0, None), 1, 0.01);
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 30, 20, false, None)], 2048);
    let shapes = painted(&ctx, &marks, &pictures);
    assert!(textured(&shapes).is_empty() && texts(&shapes).is_empty() && !shapes.is_empty(), "only the frame");
    pictures.sync(&ctx, &[hand_picture(7, 30, 20, false, Some("too large to draw"))], 2048);
    let shapes = painted(&ctx, &marks, &pictures);
    assert!(textured(&shapes).is_empty());
    assert!(texts(&shapes).iter().any(|t| t.contains("too large to draw")), "{:?}", texts(&shapes));
    assert_eq!(pictures.inspect()[0]["shown"], "failed");
}

/// A picture wider than the GPU's largest texture is split into tiles that meet edge to edge: each
/// tile's texture runs over its own columns, row 0 at the top, at the item's opacity.
#[test]
fn a_picture_past_the_gpus_side_is_split_into_tiles() {
    let ctx = egui::Context::default();
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(3.0, 1.0) };
    let marks = Marks::of(&picture_scene(7, rect, 0.5, None), 1, 0.01);
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 3, 1, true, None)], 2);
    assert_eq!(pictures.get(7).unwrap().tiles.len(), 2);
    let meshes = textured(&painted(&ctx, &marks, &pictures));
    assert_eq!(meshes.len(), 2, "one mesh a tile");
    let (a, b) = (&meshes[0], &meshes[1]);
    assert_ne!(a.0.texture_id, b.0.texture_id);
    // 10 points a millimetre: the first tile is columns 0-1 (0 to 20 points), the second column 2.
    assert!((a.1.min.x - 0.0).abs() < 1e-3 && (a.1.max.x - 20.0).abs() < 1e-3 && (b.1.min.x - 20.0).abs() < 1e-3 && (b.1.max.x - 30.0).abs() < 1e-3);
    for (mesh, _) in &meshes {
        for v in &mesh.vertices {
            // Paper y up is screen y down: the picture's top (y = 1 mm, -10 points) is the texture's row 0.
            let v_expected = if (v.pos.y + 10.0).abs() < 1e-3 { 0.0 } else { 1.0 };
            assert!((v.uv.y - v_expected).abs() < 1e-6 && (0.0..=1.0).contains(&v.uv.x), "{v:?}");
            assert_eq!(v.color, Color32::WHITE.gamma_multiply(0.5), "the item's opacity");
        }
    }
}

/// A picture in a view cut to a circle (a detail view) shows only inside the circle, as the writers
/// cut it: every corner of its mesh lies on or inside the circle and inside the picture.
#[test]
fn a_circle_clip_cuts_the_picture() {
    let ctx = egui::Context::default();
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(10.0, 10.0) };
    let clip = Clip::Circle { center: Vec2::new(5.0, 5.0), radius: 3.0 };
    let marks = Marks::of(&picture_scene(7, rect, 1.0, Some(clip)), 1, 0.01);
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 10, 10, true, None)], 2048);
    let meshes = textured(&painted(&ctx, &marks, &pictures));
    assert_eq!(meshes.len(), 1);
    let vertices = &meshes[0].0.vertices;
    assert!(vertices.len() > 16, "a circle, not the rectangle: {}", vertices.len());
    for v in vertices {
        let paper = Vec2::new(f64::from(v.pos.x) / 10.0, -f64::from(v.pos.y) / 10.0);
        assert!(paper.dist(Vec2::new(5.0, 5.0)) <= 3.0 + 1e-6, "{paper:?}");
        // The texture coordinates follow the paper: u from the left edge, v down from the top.
        assert!((f64::from(v.uv.x) - paper.x / 10.0).abs() < 1e-5 && (f64::from(v.uv.y) - (10.0 - paper.y) / 10.0).abs() < 1e-5, "{v:?}");
    }
}

/// A stale picture (its view changed, the new picture not drawn yet) keeps its last pixels where they
/// were drawn, over its own grid; a fresh one is cut to its item's place, as the export cuts it.
#[test]
fn a_stale_picture_shows_over_its_own_grid_until_the_new_one_comes() {
    let ctx = egui::Context::default();
    // The view is now 10 mm square; its last picture was drawn 20 mm square.
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(10.0, 10.0) };
    let marks = Marks::of(&picture_scene(7, rect, 1.0, None), 1, 0.01);
    let mut pictures = PictureTextures::default();
    let mut last = hand_picture(7, 20, 20, true, None);
    let width = |pictures: &PictureTextures| {
        let meshes = textured(&painted(&ctx, &marks, pictures));
        assert_eq!(meshes.len(), 1);
        meshes[0].1.width()
    };
    pictures.sync(&ctx, std::slice::from_ref(&last), 2048);
    assert!((width(&pictures) - 100.0).abs() < 1e-3, "a fresh picture is cut to its place");
    last.stale = true;
    pictures.sync(&ctx, std::slice::from_ref(&last), 2048);
    assert!((width(&pictures) - 200.0).abs() < 1e-3, "a stale one shows whole, over its own grid");
}

/// A view moved on its sheet keeps its picture's pixels (the same rows) and the engine moves their
/// grid: the page paints them at the new place, past the old one, with no new upload.
#[test]
fn a_moved_views_picture_follows_it_without_a_new_upload() {
    let ctx = egui::Context::default();
    let mut pictures = PictureTextures::default();
    let first = hand_picture(7, 10, 10, true, None);
    pictures.sync(&ctx, std::slice::from_ref(&first), 2048);
    let moved = SheetPicture { grid: PixelGrid { x0: 50.0, ..first.grid }, ..first.clone() };
    pictures.sync(&ctx, std::slice::from_ref(&moved), 2048);
    assert_eq!(pictures.uploads, 1, "the same pixels upload once");
    let rect = Aabb2 { min: Vec2::new(50.0, 0.0), max: Vec2::new(60.0, 10.0) };
    let meshes = textured(&painted(&ctx, &Marks::of(&picture_scene(7, rect, 1.0, None), 1, 0.01), &pictures));
    assert_eq!(meshes.len(), 1, "drawn at its new place");
    // 10 points a millimetre: 500 to 600 points.
    assert!((meshes[0].1.min.x - 500.0).abs() < 1e-3 && (meshes[0].1.max.x - 600.0).abs() < 1e-3, "{:?}", meshes[0].1);
}

/// The page paints in the scene's order: a fill before a picture lies under it, one after it on it.
#[test]
fn fills_and_pictures_paint_in_the_scenes_order() {
    // Whether each mesh in `shapes` is textured (a picture's) or not (a fill's), in paint order.
    fn meshes(shapes: &[egui::epaint::ClippedShape]) -> Vec<bool> {
        fn walk(shape: &egui::Shape, out: &mut Vec<bool>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                egui::Shape::Mesh(mesh) if !mesh.vertices.is_empty() => out.push(mesh.texture_id != egui::TextureId::default()),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|s| walk(&s.shape, &mut out));
        out
    }
    let ctx = egui::Context::default();
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 10, 10, true, None)], 2048);
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(10.0, 10.0) };
    let fill = Item::Fill { region: Region2 { outer: square(Vec2::new(2.0, 2.0), 5.0), holes: Vec::new() } };
    let picture = Item::Picture(solidcraft_engine::geom::paper::Picture { source: 7, rect, opacity: 1.0 });
    for (items, expected) in [(vec![fill.clone(), picture.clone()], [false, true]), (vec![picture, fill], [true, false])] {
        let groups = vec![Group { object: 1, kind: ObjectKind::View, items }];
        let scene = Scene { size_mm: Vec2::new(100.0, 100.0), groups, fonts: Vec::new(), clips: Vec::new() };
        assert_eq!(meshes(&painted(&ctx, &Marks::of(&scene, 1, 0.01), &pictures)), expected);
    }
}

/// How long making a sheet picture's textures takes on the window thread: 16 Mpx (the provider's
/// largest) and a view at 300 dpi on A3. Ignored: a measure, not a check (`cargo test -- --ignored`).
#[test]
#[ignore]
fn picture_upload_timing() {
    let ctx = egui::Context::default();
    // A desktop GPU's largest side, so the 16 Mpx picture is one texture, as `sync` is told.
    let _ = ctx.run_ui(egui::RawInput { max_texture_side: Some(16_384), ..Default::default() }, |_| {});
    for (w, h) in [(4096u32, 4096u32), (1181, 945)] {
        let p = hand_picture(1, w, h, true, None);
        let start = std::time::Instant::now();
        let mut pictures = PictureTextures::default();
        pictures.sync(&ctx, std::slice::from_ref(&p), 16_384);
        println!("picture {w} x {h}: upload {:.1} ms", start.elapsed().as_secs_f64() * 1e3);
        // Each frame after: the same answer again, and its meshes (one tile, then a circle clip).
        let start = std::time::Instant::now();
        for _ in 0..100 {
            pictures.sync(&ctx, std::slice::from_ref(&p), 16_384);
        }
        println!("  sync of the same answer: {:.2} us a frame", start.elapsed().as_secs_f64() * 1e4);
        let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(f64::from(w), f64::from(h)) };
        let at = OnScreen { centre: Pos2::ZERO, target: Vec2::ZERO, scale: 0.1 };
        let u = pictures.get(1).unwrap();
        for clip in [None, Some(Clip::Circle { center: rect.center(), radius: f64::from(h) / 3.0 })] {
            let start = std::time::Instant::now();
            for _ in 0..100 {
                for tile in &u.tiles {
                    std::hint::black_box(tile_mesh(&at, &u.grid, tile, rect, clip.as_ref(), 1.0, 0.02));
                }
            }
            println!("  meshes, clip {clip:?}: {:.2} us a frame", start.elapsed().as_secs_f64() * 1e4);
        }
    }
}

/// A final synchronous sketch solve is painted in the frame that completes it, without another
/// pointer event or an extra layout pass to make the finished marks visible.
#[test]
fn a_single_sheet_sketch_is_painted_when_its_last_step_finishes() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let drawing = h.app.run("drawing.create", json!({})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    h.frames(2);
    let made = h.app.run("drawing.sketch.create", json!({"owner": drawing["sheets"][0]["id"]})).unwrap();
    h.app.run("sketch.line", json!({"points": [[15, 25], [65, 25]], "infer": false})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let (doc, undo) = (h.app.session.doc.to_json(), h.app.session.undo.len());
    h.frame();
    let sheet = active_sheet(&h.app).unwrap();
    let current = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    assert!(current.stale_sketches.is_empty(), "the single sketch has finished");
    assert!(
        current
            .scene
            .groups
            .iter()
            .any(|group| { group.kind == ObjectKind::Sketch && json!(solidcraft_engine::doc::ids::encode(group.object)) == made["sketch"] })
    );
    let marks = h.app.drawing_workspace.page.marks.as_ref().unwrap();
    assert_eq!(marks.generation, current.generation, "the page painted the completed scene in this frame");
    assert!(marks.marks.iter().any(|mark| matches!(mark, Mark::Line { points, .. }
        if points.as_slice() == [Vec2::new(15.0, 25.0), Vec2::new(65.0, 25.0)])));
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
}

/// A view's state is in words beside it: one whose settings changed says it needs an update until it
/// is drawn again, and one with nothing to show says why.
#[test]
fn a_views_state_is_in_words() {
    let (mut h, front) = plate_sheet();
    h.app.run("drawing.view.edit", json!({"view": front, "set": {"scale": "1:5"}})).unwrap();
    // The first frame after the edit: the view is drawn beside the window from now on.
    h.frame();
    assert!(crate::scenario::rect_of(&format!("sheet:view:{front}:state")).is_some(), "its words are beside it");
    let p = inspect_now(&h);
    let state = p["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(front)).unwrap().clone();
    assert_eq!(state["out_of_date"], true, "{p}");
    assert_eq!(state["words"], json!(["Needs update"]), "{p}");
    assert_eq!(p["working"], true);
    settle(&mut h);
    let p = page(&mut h);
    assert!(p["views"].as_array().unwrap().iter().all(|v| v["out_of_date"] == false), "{p}");

    // A view with nothing to draw, or one that can't be drawn, says why, out of date or not.
    use solidcraft_engine::drawing_view::Unmeasured;
    let nothing = words(&Status::Placeholder(Unmeasured::NothingChosen), false);
    assert_eq!(nothing, [format!("Nothing to draw: {}", Unmeasured::NothingChosen)]);
    let failed = words(&Status::Failed("too many lines".into()), true);
    assert_eq!(failed, ["Can't draw this view: too many lines", "Needs update"]);
    assert!(words(&Status::Drawn, false).is_empty());
}

/// Without a drawing open the page shows the first-sheet start (#18159), and the control channel's
/// sheet page is empty; outside Drawing there is none either.
#[test]
fn no_drawing_open_shows_the_note() {
    let mut h = Harness::new();
    h.app.home.open = false;
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    h.frames(2);
    assert_eq!(page(&mut h), Value::Null);
    assert!(h.widget("Set up your drawing").is_some());
    let (mut h, _) = plate_sheet();
    crate::drawing_workspace::enter(&mut h.app, false).unwrap();
    h.frames(2);
    assert_eq!(page(&mut h), Value::Null);
}

#[test]
fn offscreen_arrow_tracks_the_edge_and_hides_when_paper_is_visible() {
    let area = Rect::from_min_max(pos2(50.0, 20.0), pos2(250.0, 220.0));
    let sheet_at = |x, y| Rect::from_center_size(pos2(x, y), vec2(30.0, 40.0));
    let (centre, direction) = offscreen_arrow(area, sheet_at(350.0, 120.0), 20.0).unwrap();
    assert_eq!(centre, pos2(230.0, 120.0));
    assert_eq!(direction, vec2(1.0, 0.0));

    let (lower, lower_direction) = offscreen_arrow(area, sheet_at(350.0, 200.0), 20.0).unwrap();
    assert_eq!(lower.x, centre.x);
    assert!(lower.y > centre.y);
    assert!(lower_direction.x > 0.0 && lower_direction.y > 0.0);

    let (above, up) = offscreen_arrow(area, sheet_at(150.0, -60.0), 20.0).unwrap();
    assert_eq!(above, pos2(150.0, 40.0));
    assert_eq!(up, vec2(0.0, -1.0));

    assert!(offscreen_arrow(area, sheet_at(260.0, 120.0), 20.0).is_none());
    assert!(offscreen_arrow(area, sheet_at(350.0, 120.0), 110.0).is_none());
}
