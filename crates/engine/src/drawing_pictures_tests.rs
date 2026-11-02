use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::paper::Scene;

use super::*;
use crate::Session;
use crate::drawing_scene::tests::front_and_top;
use crate::schema::drawing_scene::SheetSceneResult;
use crate::schema::output_schema_for;
use crate::schema::tests::conforms;
use crate::tests::Folder;

/// A whole PNG of two pixels: red, then blue.
const RED_BLUE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";

fn red_blue() -> Vec<u8> {
    base64_decode(RED_BLUE).unwrap()
}

/// A PNG's header, saying these pixels.
pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0]);
    b
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}

/// Places `bytes` on sheet `sheet`, and returns the placement's id.
fn place(s: &mut Session, folder: &Folder, bytes: &[u8], sheet: u64, rect_mm: [f64; 4]) -> u64 {
    let file = folder.path("image.png");
    std::fs::write(&file, bytes).unwrap();
    let reply = run(s, "drawing.image.add", json!({"sheet": ids::encode(sheet), "file": file, "rect_mm": rect_mm}));
    ids::decode(reply["image"].as_str().unwrap()).unwrap()
}

fn another_sheet(s: &mut Session) -> u64 {
    let added = run(s, "drawing.sheet.add", json!({}));
    ids::decode(added["sheet"].as_str().unwrap()).unwrap()
}

/// The groups of `scene` by kind, in paint order, each run of one kind once.
fn kinds(scene: &Scene) -> Vec<ObjectKind> {
    let mut kinds: Vec<ObjectKind> = scene.groups.iter().map(|group| group.kind).collect();
    kinds.dedup();
    kinds
}

/// `picture`'s own grid at 25.4 dpi, a pixel to the millimetre, and all of it as a window.
fn whole(picture: &Picture) -> (PixelGrid, GridWindow) {
    let grid = picture.grid(25.4).unwrap();
    let window = grid.window(picture.rect).unwrap();
    (grid, window)
}

/// An image sits under the views, on its own rectangle, on its own sheet: the scene says so, as its
/// schema does, and the SVG holds the file's own pixels there.
#[test]
fn an_image_draws_under_the_views_on_its_rectangle() {
    let folder = Folder::new("image-on-paper");
    let (mut s, sheet, _) = front_and_top();
    // At 300 dpi, these are exactly 300 pixels by 150.
    let id = place(&mut s, &folder, &red_blue(), sheet, [10.0, 20.0, 35.4, 32.7]);
    let elsewhere = another_sheet(&mut s);
    let d = s.drawing("test").unwrap().clone();

    let built = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    assert_eq!(kinds(&built.scene), [ObjectKind::DrawingBorder, ObjectKind::SheetImage, ObjectKind::View]);
    let group = built.scene.groups.iter().find(|group| group.kind == ObjectKind::SheetImage).unwrap();
    let picture = Picture { source: id, rect: Aabb2 { min: Vec2::new(10.0, 20.0), max: Vec2::new(35.4, 32.7) }, opacity: 1.0 };
    assert_eq!((group.object, &group.items), (id, &vec![Item::Picture(picture)]));
    let empty = crate::drawing_scene::build(&s, &d, elsewhere).unwrap();
    assert!(!empty.scene.groups.iter().any(|group| group.kind == ObjectKind::SheetImage));

    let reply = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");

    let reply = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    let svg = reply["svg"].as_str().unwrap();
    assert_eq!(svg.matches("<image ").count(), 1, "{}", &svg[..200]);
    // On the A3 sheet, whose y runs down in an SVG: 297 - 32.7.
    let at = "<image x=\"10\" y=\"264.3\" width=\"25.4\" height=\"12.7\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,";
    let (_, data) = svg.split_once(at).unwrap_or_else(|| panic!("no {at} in {}", &svg[svg.find("<image ").unwrap()..][..150]));
    let (data, _) = data.split_once('"').unwrap();
    let pixels = image::load_from_memory(&base64_decode(data).unwrap()).unwrap().into_rgba8();
    assert_eq!(pixels.dimensions(), (300, 150));
    assert_eq!((pixels.get_pixel(0, 0).0, pixels.get_pixel(149, 149).0), (RED, RED));
    assert_eq!((pixels.get_pixel(150, 0).0, pixels.get_pixel(299, 149).0), (BLUE, BLUE));
    assert!(!run(&mut s, "drawing.export", json!({"sheet": ids::encode(elsewhere)}))["svg"].as_str().unwrap().contains("<image "));
}

/// A file that doesn't decode refuses the sheet it sits on, naming the placement, and writes
/// nothing. Another sheet still exports.
#[test]
fn a_file_that_wont_decode_refuses_its_sheet_by_name() {
    let folder = Folder::new("image-wont-decode");
    let (mut s, sheet, _) = front_and_top();
    let elsewhere = another_sheet(&mut s);
    // A PNG's header alone: what a placement reads, and no decoder takes.
    let id = place(&mut s, &folder, &png(2, 1), sheet, [10.0, 20.0, 60.0, 45.0]);
    let path = folder.path("sheet.svg");
    let p = json!({"sheet": ids::encode(sheet), "path": path});
    let why = s.execute("drawing.export", &p).err().unwrap().to_string();
    assert!(why.contains(&ids::encode(id)) && why.contains("not a whole PNG or JPEG"), "{why}");
    assert!(!std::path::Path::new(&path).exists());
    run(&mut s, "drawing.export", json!({"sheet": ids::encode(elsewhere)}));
}

/// Two placements of one file are one decode, each drawing on its own rectangle. A provider decodes
/// nothing until a writer asks.
#[test]
fn two_placements_of_a_file_decode_it_once() {
    let folder = Folder::new("image-twice");
    let (mut s, sheet, _) = front_and_top();
    let first = place(&mut s, &folder, &red_blue(), sheet, [10.0, 20.0, 12.0, 21.0]);
    let second = place(&mut s, &folder, &red_blue(), sheet, [100.0, 50.0, 102.0, 51.0]);
    let d = s.drawing("test").unwrap().clone();
    let provider = Provider::new(&d, sheet).unwrap();
    assert_eq!(provider.files.len(), 1);
    let cell = &provider.files.values().next().unwrap().1;
    assert!(cell.get().is_none());
    let groups = groups(&d, sheet);
    let mut by_id = [first, second];
    by_id.sort_unstable();
    assert_eq!(groups.iter().map(|group| group.object).collect::<Vec<_>>(), by_id);
    for group in &groups {
        let Item::Picture(picture) = &group.items[0] else { panic!("{group:?}") };
        let (grid, window) = whole(picture);
        assert_eq!(provider.rows(group.object, &grid, window).unwrap(), [RED, BLUE].concat());
    }
    assert!(cell.get().is_some());
}

/// A placement on another sheet isn't this sheet's, and an unknown id is an error, never a panic.
#[test]
fn a_provider_answers_only_for_its_sheet() {
    let folder = Folder::new("image-its-sheet");
    let (mut s, sheet, _) = front_and_top();
    let elsewhere = another_sheet(&mut s);
    let other = place(&mut s, &folder, &red_blue(), elsewhere, [10.0, 20.0, 12.0, 21.0]);
    let d = s.drawing("test").unwrap().clone();
    let provider = Provider::new(&d, sheet).unwrap();
    assert!(provider.files.is_empty());
    let group = groups(&d, elsewhere).remove(0);
    let Item::Picture(picture) = &group.items[0] else { panic!("{group:?}") };
    let (grid, window) = whole(picture);
    let why = provider.rows(other, &grid, window).unwrap_err();
    assert!(why.contains(&ids::encode(other)), "{why}");
}

/// The cap is on the files a sheet uses, from their headers, each file once: at the cap is taken, one
/// pixel over is refused before anything decodes, and another sheet's files don't count.
#[test]
fn a_sheets_files_are_capped_from_their_headers() {
    let folder = Folder::new("image-cap");
    let (mut s, sheet, _) = front_and_top();
    let elsewhere = another_sheet(&mut s);
    let rect = [10.0, 20.0, 60.0, 45.0];
    // 8,000 × 8,000 is the cap, and placing it twice is still one file.
    place(&mut s, &folder, &png(8000, 8000), sheet, rect);
    place(&mut s, &folder, &png(8000, 8000), sheet, rect);
    place(&mut s, &folder, &png(1, 1), elsewhere, rect);
    let d = s.drawing("test").unwrap().clone();
    assert!(Provider::new(&d, sheet).is_ok());
    place(&mut s, &folder, &png(1, 1), sheet, rect);
    let d = s.drawing("test").unwrap().clone();
    let why = Provider::new(&d, sheet).err().unwrap();
    assert!(why.contains("64000001"), "{why}");
    // The export says that, and not that a header-only file won't decode.
    let why = s.execute("drawing.export", &json!({"sheet": ids::encode(sheet)})).err().unwrap().to_string();
    assert!(why.contains("64000001"), "{why}");
}

/// The sheet page's provider redraws for a new placement and for its undo, and its scene is the one
/// a build draws.
#[test]
fn the_providers_scene_follows_the_placements() {
    let folder = Folder::new("image-provider");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 40, "height": 30}));
    run(&mut s, "drawing.create", json!({}));
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    let before = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert_eq!(kinds(&before.scene), [ObjectKind::DrawingBorder]);

    place(&mut s, &folder, &red_blue(), sheet, [10.0, 20.0, 60.0, 45.0]);
    let placed = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert_ne!(placed.generation, before.generation);
    assert_eq!(kinds(&placed.scene), [ObjectKind::DrawingBorder, ObjectKind::SheetImage]);
    let d = s.drawing("test").unwrap().clone();
    assert_eq!(*placed.scene, crate::drawing_scene::build(&s, &d, sheet).unwrap().scene);
    let same = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert!(Arc::ptr_eq(&same.scene, &placed.scene));

    run(&mut s, "edit.undo", json!({}));
    let undone = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert_eq!(kinds(&undone.scene), [ObjectKind::DrawingBorder]);
}
