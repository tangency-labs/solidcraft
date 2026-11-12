use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
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
    let pins = |s: &Session| s.world_state().unwrap().bodies.iter().map(|b| b.component == pin).collect::<Vec<bool>>();
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
        json!({"sheet": "Sheet 1", "view": "front", "scale": scale, "position_mm": [200, 150], "visual_style": "shaded"}),
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

/// The limit bounds the whole view, not each request: a writer's bands add up, the next one is
/// refused by name before it paints, and another view's count is its own.
#[test]
fn a_views_requests_add_up_to_its_limit() {
    let (s, sheet, id) = notched("1:1");
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    let group = built.scene.groups.iter().find(|g| g.object == id).unwrap();
    let Some(Item::Picture(picture)) = group.items.first() else { panic!("{:?}", group.items) };
    let grid = picture.grid(300.0).unwrap();
    let band = |y0: u32, y1: u32| GridWindow { x0: 0, y0, x1: grid.width, y1 };
    let middle = grid.height / 2;
    let counted = |window: GridWindow| {
        let pictures = ViewPictures::new(&s, &d, sheet).unwrap();
        pictures.rows(id, &grid, window).unwrap();
        pictures.spent[&id].get()
    };
    let (top, bottom) = (counted(band(0, middle)), counted(band(middle, grid.height)));
    assert!(top > 0 && bottom > 0, "{top} {bottom}");

    let limited = |max_pixel_tests: u64| ViewPictures { max_pixel_tests, ..ViewPictures::new(&s, &d, sheet).unwrap() };
    let exact = limited(top + bottom);
    exact.rows(id, &grid, band(0, middle)).unwrap();
    exact.rows(id, &grid, band(middle, grid.height)).unwrap();
    let under = limited(top + bottom - 1);
    under.rows(id, &grid, band(0, middle)).unwrap();
    let why = under.rows(id, &grid, band(middle, grid.height)).unwrap_err();
    assert!(why.contains(&format!("exceed {} pixel tests", top + bottom - 1)) && why.contains(&ids::encode(id)), "{why}");
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
    let p = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [200, 150], "visual_style": "shaded",
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
    let p = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [200, 150], "visual_style": "shaded",
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
        json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [210, 150], "visual_style": "shaded"}),
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
    let (two, count) = items(&mut s, json!({"visual_style": "shaded", "reference_style": "shaded", "reference_hides_model": false}));
    assert_eq!((two, count), (vec![id ^ UNDER, id], 2));
    assert_eq!(items(&mut s, json!({"reference_hides_model": true})), (vec![id], 1));
    let (model, count) = items(&mut s, json!({"reference_style": "visible", "reference_hides_model": false}));
    assert!(model == [id] && count > 1, "{model:?} {count}");
    let (reference, count) = items(&mut s, json!({"visual_style": "visible", "reference_style": "shaded"}));
    assert!(reference == [id] && count > 1, "{reference:?} {count}");
    assert_eq!(items(&mut s, json!({"reference_style": "visible"})).0, Vec::<u64>::new());
}

/// Component A, a 40 mm cube at the origin, and components B and C, 20 mm cubes at x 60 and x 100,
/// with a shaded front view of all three at 1:1 on (200, 150): the session, the sheet, the view and
/// B's and C's occurrences.
fn three_parts() -> (Session, u64, u64, [Value; 2]) {
    let mut s = Session::default();
    let mut part = |name: &str, size: f64, corner: [f64; 3]| {
        let occurrence = run(&mut s, "component.create", json!({"name": name}))["occurrence"].clone();
        run(&mut s, "solid.box", json!({"length": size, "width": size, "height": size, "corner": corner, "operation": "new"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        occurrence
    };
    part("A", 40.0, [0.0, 0.0, 0.0]);
    let parts = [part("B", 20.0, [60.0, 0.0, 0.0]), part("C", 20.0, [100.0, 0.0, 0.0])];
    run(&mut s, "drawing.create", json!({}));
    let p = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [200, 150], "visual_style": "shaded"});
    let id = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    (s, sheet, id, parts)
}

/// A view's picture draws what its lines draw, round where the view was placed: all three parts;
/// B hidden, which leaves its place between A and C empty; C hidden, which centres A and B; then B
/// alone as the view's source. B comes after A in the design, so its picture only shows when each
/// part keeps its own place among the design's bodies.
#[test]
fn a_views_picture_draws_only_what_it_shows() {
    let (mut s, sheet, id, [b, c]) = three_parts();
    let edit = |s: &mut Session, set: Value| run(s, "drawing.view.edit", json!({"view": ids::encode(id), "set": set}));
    let picture = |s: &Session| {
        let (rect, grid, rows) = drawn(s, sheet, id);
        (rect, move |x_mm: f64, y_mm: f64| alpha(&grid, &rows, x_mm, y_mm))
    };
    let near = |rect: Aabb2, min: [f64; 2], max: [f64; 2]| {
        rect.min.dist(Vec2::new(min[0], min[1])) < 1e-9 && rect.max.dist(Vec2::new(max[0], max[1])) < 1e-9
    };

    let (rect, at) = picture(&s);
    assert!(near(rect, [140.0, 130.0], [260.0, 170.0]), "all three: {rect:?}");
    assert_eq!((at(160.0, 150.0), at(210.0, 140.0), at(250.0, 140.0), at(210.0, 165.0)), (255, 255, 255, 0), "A, B, C, and nothing above B");

    edit(&mut s, json!({"hide": [[b]]}));
    let (rect, at) = picture(&s);
    assert!(near(rect, [140.0, 130.0], [260.0, 170.0]), "A and C span the same: {rect:?}");
    assert_eq!((at(160.0, 150.0), at(210.0, 140.0), at(250.0, 140.0)), (255, 0, 255), "A, nothing where B was, C");

    edit(&mut s, json!({"hide": [[c]]}));
    let (rect, at) = picture(&s);
    assert!(near(rect, [160.0, 130.0], [240.0, 170.0]), "A and B: {rect:?}");
    assert_eq!((at(165.0, 150.0), at(195.0, 150.0), at(230.0, 140.0)), (255, 255, 255), "A's left and right sides, and B");

    edit(&mut s, json!({"hide": null, "source": {"occurrences": [[b]]}}));
    let (rect, at) = picture(&s);
    assert!(near(rect, [190.0, 140.0], [210.0, 160.0]), "B alone: {rect:?}");
    assert_eq!((at(192.0, 142.0), at(200.0, 150.0), at(208.0, 158.0)), (255, 255, 255), "B, corner to corner");
}

/// A view that hides its reference parts has no picture of them: the views' pictures are the ones
/// its scene asks for, the model's alone.
#[test]
fn hiding_the_reference_parts_leaves_their_picture_out() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plate", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let block = run(&mut s, "component.create", json!({"name": "Block"}))["occurrence"].clone();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [90, 20, 20], "body_name": "Block", "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.metadata.set", json!({"component": "Block", "bom_type": "reference"}));
    run(&mut s, "drawing.create", json!({}));
    let p = json!({"sheet": "Sheet 1", "view": "top", "scale": "1:1", "position_mm": [150, 150], "visual_style": "shaded",
                   "reference_style": "shaded", "reference_hides_model": false, "hide": [[block]]});
    let id = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    let group = built.scene.groups.iter().find(|g| g.object == id).unwrap();
    let sources: Vec<u64> = group.items.iter().filter_map(|i| if let Item::Picture(p) = i { Some(p.source) } else { None }).collect();
    assert_eq!(sources, [id]);
    let pictures = ViewPictures::new(&s, &d, sheet).unwrap();
    assert!(pictures.has(id) && !pictures.has(id ^ UNDER), "the hidden reference parts' picture");
}

/// A drawing file can hold a shaded view of an exploded view, which the commands refuse to make
/// (#14531): the sheet marks it failed with why and draws nothing for it, and it has no picture of the
/// parts where the design places them.
#[test]
fn a_shaded_exploded_view_from_a_file_draws_no_lines_and_no_picture() {
    let mut s = Session::default();
    let mut part = |name: &str, corner: [f64; 3]| {
        let occurrence = run(&mut s, "component.create", json!({"name": name}))["occurrence"].clone();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": corner, "operation": "new"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        occurrence.as_u64().unwrap()
    };
    part("A", [0.0, 0.0, 0.0]);
    let b = part("B", [30.0, 0.0, 0.0]);
    let made = run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": b, "translate": [30, 0, 0]}]}));
    let apart = made["view"].as_u64().unwrap();
    run(&mut s, "drawing.create", json!({}));
    let p = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [200, 150], "visual_style": "shaded"});
    let id = ids::decode(run(&mut s, "drawing.view.base", p)["view"].as_str().unwrap()).unwrap();
    // The file's record: the shaded view draws the exploded view.
    let mut d = (**s.drawing("test").unwrap()).clone();
    let at = d.views.iter().position(|v| v.id == id).unwrap();
    std::sync::Arc::make_mut(&mut d.views[at]).settings.exploded = Some(solidcraft_doc::drawing_view::Exploded::View(apart));
    s.doc_mut().put_drawing(std::sync::Arc::new(d));

    let d = s.drawing("test").unwrap().clone();
    let sheet = d.sheets[0].id;
    let built = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    let status = built.views.iter().find(|v| v.view == id).map(|v| v.status.clone());
    let why = "not supported yet: a shaded view of an exploded view";
    assert_eq!(status, Some(crate::drawing_scene::Status::Failed(why.into())));
    assert!(!built.scene.groups.iter().any(|g| g.object == id), "nothing drawn for it: {:?}", built.scene.groups);
    assert!(!ViewPictures::new(&s, &d, sheet).unwrap().has(id), "no picture of the parts as the design places them");
}

/// A view's picture lists only the bodies it draws, each by its place among the design's bodies and
/// with its role: B hidden leaves A and C; C as a reference part with lines only that hides the model
/// hides without painting; C shaded under the model is a picture of its own, left out of the model's.
#[test]
fn a_views_picture_keeps_only_the_bodies_it_draws() {
    use solidcraft_render::job::BodyRole::{DepthOnly, Painted};
    let (mut s, sheet, id, [b, c]) = three_parts();
    let shown = crate::drawing_view::shown(&s).unwrap();
    let place = |occurrence: &Value| {
        let occurrence = occurrence.as_u64().unwrap();
        shown.bodies.iter().find(|body| body.path == [occurrence]).unwrap().index
    };
    let (b_at, c_at) = (place(&b), place(&c));
    assert_eq!(shown.bodies.len(), 3);
    let a_at = (0..3).find(|at| ![b_at, c_at].contains(at)).unwrap();
    assert!(a_at < b_at && b_at < c_at, "B lies between A and C among the bodies: {a_at} {b_at} {c_at}");
    let bodies = |s: &Session, source: u64| {
        let d = s.drawing("test").unwrap().clone();
        ViewPictures::new(s, &d, sheet).unwrap().pictures.get(&source).map(|picture| picture.bodies.clone())
    };
    let edit = |s: &mut Session, set: Value| run(s, "drawing.view.edit", json!({"view": ids::encode(id), "set": set}));

    edit(&mut s, json!({"hide": [[b]]}));
    assert_eq!(bodies(&s, id), Some(vec![(a_at, Painted), (c_at, Painted)]), "B hidden");

    let drawing = ids::encode(s.drawing("test").unwrap().id);
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "occurrence.bom_type", json!({"occurrence": c, "bom_type": "reference"}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    edit(&mut s, json!({"hide": null, "reference_style": "visible", "reference_hides_model": true}));
    assert_eq!(bodies(&s, id), Some(vec![(a_at, Painted), (b_at, Painted), (c_at, DepthOnly)]), "C hides with its lines only");

    edit(&mut s, json!({"reference_style": "shaded", "reference_hides_model": false}));
    assert_eq!(bodies(&s, id ^ UNDER), Some(vec![(c_at, Painted)]), "C's own picture, under the model's");
    assert_eq!(bodies(&s, id), Some(vec![(a_at, Painted), (b_at, Painted)]), "the model's, without C");
}
