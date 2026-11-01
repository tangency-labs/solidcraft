use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_render::VisualStyle::{Shaded, Visible, VisibleHidden};

use super::*;
use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Each case of the two styles and the hide setting, for a model body and a reference part:
/// the pictures bottom first, as (model body, reference part).
#[test]
fn each_style_and_hide_setting_gives_its_pictures() {
    use Part::{DepthOnly, Left, Painted};
    let both = [false, true];
    let cases = [
        (Visible, VisibleHidden, false, vec![]),
        (Visible, Visible, true, vec![]),
        (Shaded, Shaded, true, vec![(Painted, Painted)]),
        (Shaded, Shaded, false, vec![(Left, Painted), (Painted, Left)]),
        (Shaded, Visible, true, vec![(Painted, DepthOnly)]),
        (Shaded, VisibleHidden, false, vec![(Painted, Left)]),
        (Visible, Shaded, false, vec![(DepthOnly, Painted)]),
        (VisibleHidden, Shaded, true, vec![(DepthOnly, Painted)]),
    ];
    for (display, reference, hides, expected) in cases {
        let got: Vec<(Part, Part)> = pictures(display, reference, hides, &both).into_iter().map(|p| (p[0], p[1])).collect();
        assert_eq!(got, expected, "{display:?} {reference:?} hides {hides}");
    }
    // A picture with nothing painted in it is left out: shaded references, none there.
    assert_eq!(pictures(Visible, Shaded, false, &[false, false]), Vec::<Vec<Part>>::new());
    assert_eq!(pictures(Shaded, Shaded, false, &[false]), [vec![Painted]], "no reference parts: the model's picture alone");
    assert_eq!(pictures(Shaded, Visible, false, &[true]), Vec::<Vec<Part>>::new(), "reference parts alone, lines only");
}

/// A plate placed at the top, and a hinge placed twice holding a pin: with the hinge a reference
/// component, both pins are reference parts and the plate isn't; with the pin itself one, the
/// same; with none, nothing is.
#[test]
fn bodies_placed_through_a_reference_component_are_reference_parts() {
    let reference_flags = |s: &Session| crate::drawing_view::shown(s).unwrap().bodies.iter().map(|b| b.reference).collect::<Vec<bool>>();
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Hinge", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Pin", "parent": "Hinge"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": "Hinge", "translate": [0, 50, 0]}));
    let pin = s.doc.find_component("Pin").unwrap();
    let pins = |s: &Session| s.world_state().bodies.iter().map(|b| b.component == pin).collect::<Vec<bool>>();
    assert_eq!(pins(&s).iter().filter(|p| **p).count(), 2, "{:?}", pins(&s));
    assert_eq!(reference_flags(&s), vec![false; pins(&s).len()]);
    for reference in ["Hinge", "Pin"] {
        run(&mut s, "component.metadata.set", json!({"component": reference, "bom_type": "reference"}));
        assert_eq!(reference_flags(&s), pins(&s), "{reference}");
        run(&mut s, "component.metadata.set", json!({"component": reference, "bom_type": "normal"}));
    }
    run(&mut s, "component.metadata.set", json!({"component": "Plate", "bom_type": "reference"}));
    assert_eq!(reference_flags(&s), pins(&s).iter().map(|p| !p).collect::<Vec<bool>>(), "the plate alone");
}

/// A block 100 x 50 x 20 mm with a 40 x 20 x 10 mm notch out of its top right corner, seen from the
/// front, so a flip shows, and a drawing with a shaded view of it at `scale` on (200, 150): the
/// session, the sheet and the view.
fn notched(scale: &str) -> (Session, u64, u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 60, "height": 15, "corner": [60, -5, 10], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "drawing.create", json!({}));
    let view = run(
        &mut s,
        "drawing.view.base",
        json!({"sheet": "Sheet 1", "view": "front", "scale": scale, "position_mm": [200, 150], "display_style": "shaded"}),
    );
    let id = ids::decode(view["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    (s, sheet, id)
}

fn pixels_of(s: &Session, sheet: u64, id: u64, grid: &PixelGrid, window: GridWindow) -> Vec<u8> {
    let d = s.drawing("test").unwrap().clone();
    ViewPictures::new(s, &d, sheet).unwrap().rows(id, grid, window).unwrap()
}

/// The sheet's scene: the view's group and its picture's grid and pixels.
fn drawn(s: &Session, sheet: u64, id: u64) -> (Aabb2, PixelGrid, Vec<u8>) {
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(s, &d, sheet).unwrap();
    let group = built.scene.groups.iter().find(|g| g.object == id).unwrap();
    let Some(Item::Picture(picture)) = group.items.first() else { panic!("{:?}", group.items) };
    let grid = picture.grid(300.0).unwrap();
    let whole = GridWindow { x0: 0, y0: 0, x1: grid.width, y1: grid.height };
    (picture.rect, grid, pixels_of(s, sheet, id, &grid, whole))
}

/// A picture's alpha at a sheet position.
fn alpha(grid: &PixelGrid, rows: &[u8], x_mm: f64, y_mm: f64) -> u8 {
    let (x, y) = (((x_mm - grid.x0) / grid.pitch) as usize, grid.height as usize - 1 - ((y_mm - grid.y0) / grid.pitch) as usize);
    rows[(y * grid.width as usize + x) * 4 + 3]
}

/// The picture lands on the view's lines: its rectangle is the view's boundary at either scale and
/// on a sheet position that isn't round, to 1e-9 mm, and its pixels show the notch at the top right
/// and not the top left.
#[test]
fn a_views_picture_lies_on_its_lines() {
    for (scale, factor) in [("1:1", 1.0), ("1:2", 0.5)] {
        let (s, sheet, id) = notched(scale);
        let (rect, grid, rows) = drawn(&s, sheet, id);
        let want =
            Aabb2 { min: Vec2::new(200.0 - 50.0 * factor, 150.0 - 10.0 * factor), max: Vec2::new(200.0 + 50.0 * factor, 150.0 + 10.0 * factor) };
        assert!(rect.min.dist(want.min) < 1e-9 && rect.max.dist(want.max) < 1e-9, "{rect:?}");
        let alpha = |x_mm: f64, y_mm: f64| alpha(&grid, &rows, x_mm, y_mm);
        assert_eq!(alpha(want.min.x + 5.0 * factor, want.max.y - 5.0 * factor), 255, "top left: the block");
        assert_eq!(alpha(want.max.x - 5.0 * factor, want.max.y - 5.0 * factor), 0, "top right: the notch");
        assert_eq!(alpha(want.max.x - 5.0 * factor, want.min.y + 5.0 * factor), 255, "bottom right: the block");
        // The notch's wall, a pixel's width each side: the pixels are where the lines are.
        let (wall, y) = (want.min.x + 60.0 * factor, want.max.y - 5.0 * factor);
        assert_eq!(alpha(wall - 1.5 * grid.pitch, y), 255, "left of the wall");
        assert_eq!(alpha(wall + 1.5 * grid.pitch, y), 0, "right of the wall");
        assert!(rows.iter().skip(3).step_by(4).any(|a| *a > 0 && *a < 255), "the wall isn't on a pixel's edge, so it's smoothed");
    }
}

/// A reference part grows what a view shows, on its own side, and the picture stays on the lines:
/// the model's middle is where the view was placed, and the rectangle is the boundary.
#[test]
fn references_grow_a_picture_and_it_stays_on_the_lines() {
    let mut s = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut s, "component.create", json!({"name": name}))["occurrence"].clone();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Under", "BodyUnder");
    let over = part("Over", "BodyOver");
    run(&mut s, "occurrence.move", json!({"occurrence": over, "translate": [10, 0, 10]}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    run(&mut s, "drawing.create", json!({}));
    let p = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [200, 150], "display_style": "shaded",
                   "reference_style": "shaded", "reference_shown_percent": 100});
    let id = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let (rect, grid, model) = drawn(&s, sheet, id);
    let want = Aabb2 { min: Vec2::new(190.0, 145.0), max: Vec2::new(220.0, 165.0) };
    assert!(rect.min.dist(want.min) < 1e-9 && rect.max.dist(want.max) < 1e-9, "{rect:?}");
    let (pitch, whole) = (grid.pitch, GridWindow { x0: 0, y0: 0, x1: grid.width, y1: grid.height });
    // The model on top: 20 x 10 mm round where the view was placed.
    let at = |x_mm: f64, y_mm: f64| alpha(&grid, &model, x_mm, y_mm);
    assert_eq!((at(200.0, 150.0), at(210.0 - 1.5 * pitch, 150.0), at(210.0 + 1.5 * pitch, 150.0)), (255, 255, 0), "the model's right side");
    assert_eq!((at(200.0, 155.0 - 1.5 * pitch), at(200.0, 155.0 + 1.5 * pitch)), (255, 0), "the model's top");
    // The reference part under it: 10 along and 10 up.
    let reference = pixels_of(&s, sheet, id ^ UNDER, &grid, whole);
    let at = |x_mm: f64, y_mm: f64| alpha(&grid, &reference, x_mm, y_mm);
    assert_eq!((at(210.0, 160.0), at(200.0 + 1.5 * pitch, 160.0), at(200.0 - 1.5 * pitch, 160.0)), (255, 255, 0), "the reference part's left side");
    assert_eq!((at(210.0, 155.0 + 1.5 * pitch), at(210.0, 155.0 - 1.5 * pitch)), (255, 0), "the reference part's bottom");
}

/// A lines-only reference part in front of the model, hiding it, leaves a hole in the model's picture
/// and paints nothing of its own.
#[test]
fn a_reference_with_lines_only_hides_the_model_and_draws_nothing() {
    let mut s = Session::default();
    let mut part = |name: &str, body: &str| {
        let occurrence = run(&mut s, "component.create", json!({"name": name}))["occurrence"].clone();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": body, "operation": "new"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("Under", "BodyUnder");
    let over = part("Over", "BodyOver");
    run(&mut s, "occurrence.move", json!({"occurrence": over, "translate": [10, -30, 0]}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": over, "bom_type": "reference"}));
    run(&mut s, "drawing.create", json!({}));
    let p = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [200, 150], "display_style": "shaded",
                   "reference_style": "visible", "reference_hides_model": true});
    let id = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let (_, grid, rows) = drawn(&s, sheet, id);
    assert_eq!(alpha(&grid, &rows, 195.0, 150.0), 255, "the model, clear of the reference part");
    assert_eq!(alpha(&grid, &rows, 205.0, 150.0), 0, "the model behind it, hidden, and the part itself not painted");
}

/// A window asked for whole and in bands gives the same pixels, and twice the same bytes.
#[test]
fn bands_put_together_are_the_picture_drawn_whole() {
    let (s, sheet, id) = notched("1:1");
    let (_, grid, whole) = drawn(&s, sheet, id);
    let (width, height) = (grid.width, grid.height);
    let mut bands = Vec::new();
    for (y0, y1) in [(0, height / 3), (height / 3, height / 2), (height / 2, height)] {
        bands.extend(pixels_of(&s, sheet, id, &grid, GridWindow { x0: 0, y0, x1: width, y1 }));
    }
    assert_eq!(bands, whole);
    assert_eq!(drawn(&s, sheet, id).2, whole);
}

/// A window over a seam between tiles is the same pixels as a window each side of it.
#[test]
fn tiles_put_together_are_the_picture_drawn_whole() {
    let (s, sheet, id) = notched("1:1");
    let (_, grid, whole) = drawn(&s, sheet, id);
    assert!(grid.width > TILE, "{}", grid.width);
    let (width, y1) = (grid.width as usize, grid.height);
    for (x0, x1) in [(TILE - 8, TILE), (TILE, TILE + 8)] {
        let part = pixels_of(&s, sheet, id, &grid, GridWindow { x0, y0: 0, x1, y1 });
        let expected: Vec<u8> = whole.chunks_exact(width * 4).flat_map(|row| &row[x0 as usize * 4..x1 as usize * 4]).copied().collect();
        assert_eq!(part, expected, "{x0}..{x1}");
    }
}

/// A thin view wider than a picture task's limit is drawn: a sheet at a high dpi is far wider still.
#[test]
fn a_view_wider_than_a_task_is_drawn() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 360, "width": 5, "height": 5, "body_name": "Block"}));
    run(&mut s, "drawing.create", json!({}));
    let view = run(
        &mut s,
        "drawing.view.base",
        json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [210, 150], "display_style": "shaded"}),
    );
    let id = ids::decode(view["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let (_, grid, pixels) = drawn(&s, sheet, id);
    assert!(grid.width > 4096, "{}", grid.width);
    let row = (grid.height / 2 * grid.width) as usize;
    for x in [100, 2000, 4100] {
        assert!(pixels[(row + x) * 4 + 3] > 0, "the block at column {x}");
    }
}

/// A model edit changes the pixels; hiding the body in the browser doesn't: a drawing draws every
/// body the design places.
#[test]
fn a_picture_follows_the_model_and_not_the_eye() {
    let (mut s, sheet, id) = notched("1:1");
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let before = drawn(&s, sheet, id);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "browser.visibility", json!({"items": ["Block"], "visible": false}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert_eq!(drawn(&s, sheet, id).2, before.2, "the eye counts for nothing");
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 60, "height": 15, "corner": [0, -5, 10], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let (rect, _, after) = drawn(&s, sheet, id);
    assert_eq!(rect, before.0, "the box is the same");
    assert_ne!(after, before.2);
}

/// The reference cases on a sheet: a plate and a reference block. A shaded model over shaded
/// reference parts that don't hide it is two pictures, the reference parts' under the model's, each
/// with its own source, both under the lines; with the hide on, one. Lines-only reference parts leave
/// their lines over a picture of the model.
#[test]
fn a_views_pictures_lie_under_its_lines_and_each_other() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [90, 20, 20], "body_name": "Block", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.metadata.set", json!({"component": "Block", "bom_type": "reference"}));
    run(&mut s, "drawing.create", json!({}));
    let view = run(&mut s, "drawing.view.base", json!({"sheet": "Sheet 1", "view": "top", "scale": "1:1", "position_mm": [150, 150]}));
    let id = ids::decode(view["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let items = |s: &mut Session, set: Value| -> (Vec<u64>, usize) {
        run(s, "drawing.view.edit", json!({"view": view["view"], "set": set}));
        let d = s.drawing("test").unwrap().clone();
        let built = crate::drawing_scene::build(s, &d, sheet).unwrap();
        let group = built.scene.groups.iter().find(|g| g.object == id).unwrap();
        let sources: Vec<u64> = group.items.iter().filter_map(|i| if let Item::Picture(p) = i { Some(p.source) } else { None }).collect();
        // Pictures first, then only strokes.
        assert!(group.items.iter().skip(sources.len()).all(|i| matches!(i, Item::Stroke { .. })), "{:?}", group.items);
        (sources, group.items.len())
    };
    let (two, count) = items(&mut s, json!({"display_style": "shaded", "reference_style": "shaded", "reference_hides_model": false}));
    assert_eq!((two, count), (vec![id ^ UNDER, id], 2));
    assert_eq!(items(&mut s, json!({"reference_hides_model": true})), (vec![id], 1));
    let (model, count) = items(&mut s, json!({"reference_style": "visible", "reference_hides_model": false}));
    assert!(model == [id] && count > 1, "{model:?} {count}");
    let (reference, count) = items(&mut s, json!({"display_style": "visible", "reference_style": "shaded"}));
    assert!(reference == [id] && count > 1, "{reference:?} {count}");
    assert_eq!(items(&mut s, json!({"reference_style": "visible"})).0, Vec::<u64>::new());
}
