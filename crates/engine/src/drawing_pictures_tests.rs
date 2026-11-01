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
    assert_eq!(kinds(&before.scene), [ObjectKind::DrawingBorder, ObjectKind::TitleBlock]);

    place(&mut s, &folder, &red_blue(), sheet, [10.0, 20.0, 60.0, 45.0]);
    let placed = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert_ne!(placed.generation, before.generation);
    assert_eq!(kinds(&placed.scene), [ObjectKind::DrawingBorder, ObjectKind::SheetImage, ObjectKind::TitleBlock]);
    let d = s.drawing("test").unwrap().clone();
    assert_eq!(*placed.scene, crate::drawing_scene::build(&s, &d, sheet).unwrap().scene);
    let same = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert!(Arc::ptr_eq(&same.scene, &placed.scene));

    run(&mut s, "edit.undo", json!({}));
    let undone = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert_eq!(kinds(&undone.scene), [ObjectKind::DrawingBorder, ObjectKind::TitleBlock]);
}

/// A drawing of two views, on the sheet it returns, with the built-in title block.
fn with_a_block() -> (Session, u64) {
    let (mut s, sheet, _) = front_and_top();
    let block = serde_json::to_value(solidcraft_geom::title_block::default_block()).unwrap();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": block}));
    (s, sheet)
}

/// Shows `bytes` in title block cell `cell` of every sheet, and returns the file's id.
fn logo(s: &mut Session, folder: &Folder, bytes: &[u8], cell: &str) -> u64 {
    let file = folder.path("logo.png");
    std::fs::write(&file, bytes).unwrap();
    let reply = run(s, "drawing.title_block.logo", json!({"cell": cell, "file": file}));
    ids::decode(reply["image"].as_str().unwrap()).unwrap()
}

fn block_group(scene: &Scene) -> &Group {
    scene.groups.iter().find(|group| group.kind == ObjectKind::TitleBlock).unwrap()
}

/// A logo of two pixels by one in the legal owner's cell on A3, (230, 44) to (310, 59): the value box
/// inside its lines and under its label is (231.35, 45.175) to (308.825, 54.15), 8.975 mm high, so the
/// logo is 17.95 by 8.975, centred on x = 270.0875. It comes first in the block's group, so the lines
/// draw over it; the SVG holds its own pixels. Taken away, the block is as it was.
#[test]
fn a_logo_sits_in_its_cells_value_box_under_the_lines() {
    let folder = Folder::new("logo-on-paper");
    let (mut s, sheet) = with_a_block();
    let d = s.drawing("test").unwrap().clone();
    let plain = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    logo(&mut s, &folder, &red_blue(), "legal_owner");
    let d = s.drawing("test").unwrap().clone();

    let built = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    let group = block_group(&built.scene);
    let Item::Picture(picture) = group.items[0] else { panic!("{:?}", group.items[0]) };
    let want = Aabb2 { min: Vec2::new(261.1125, 45.175), max: Vec2::new(279.0625, 54.15) };
    assert!(picture.rect.min.dist(want.min) < 1e-9 && picture.rect.max.dist(want.max) < 1e-9, "{:?}", picture.rect);
    assert_eq!(group.items[1..], block_group(&plain.scene).items[..], "the lines and the symbol are as they were");
    built.scene.validate().unwrap();

    let reply = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");

    let reply = run(&mut s, "drawing.export", json!({"sheet": ids::encode(sheet)}));
    let svg = reply["svg"].as_str().unwrap();
    assert_eq!(svg.matches("<image ").count(), 1);
    let (_, data) = svg.split_once("href=\"data:image/png;base64,").unwrap();
    let (data, _) = data.split_once('"').unwrap();
    let pixels = image::load_from_memory(&base64_decode(data).unwrap()).unwrap().into_rgba8();
    let (w, h) = pixels.dimensions();
    assert_eq!((pixels.get_pixel(1, h / 2).0, pixels.get_pixel(w - 2, h / 2).0), (RED, BLUE));

    run(&mut s, "drawing.title_block.logo", json!({"cell": "legal_owner", "file": null}));
    let d = s.drawing("test").unwrap().clone();
    let gone = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    assert_eq!(block_group(&gone.scene).items, block_group(&plain.scene).items);
}

/// One file in two cells and placed on the sheet: three sources, each with its own window on the same
/// pixels, one file decoded; no source is a placement's.
#[test]
fn a_file_in_two_cells_and_placed_is_three_pictures() {
    let folder = Folder::new("logo-twice");
    let (mut s, sheet) = with_a_block();
    logo(&mut s, &folder, &red_blue(), "legal_owner");
    logo(&mut s, &folder, &red_blue(), "title");
    let placement = place(&mut s, &folder, &red_blue(), sheet, [10.0, 20.0, 60.0, 45.0]);
    let d = s.drawing("test").unwrap().clone();
    let pictures: Vec<Picture> = logo_items(&d, sheet)
        .into_iter()
        .map(|item| match item {
            Item::Picture(picture) => picture,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(pictures.len(), 2);
    assert_ne!(pictures[0].source, pictures[1].source);
    assert!(pictures.iter().all(|picture| picture.source != placement));
    let provider = Provider::new(&d, sheet).unwrap();
    assert_eq!((provider.places.len(), provider.files.len()), (3, 1));
    for picture in &pictures {
        let (grid, window) = whole(picture);
        assert!(provider.rows(picture.source, &grid, window).is_ok());
    }
    assert!(provider.files.values().all(|(_, decoded)| decoded.get().is_some()));
}

/// A logo's source steps over a placement's id, however small the id.
#[test]
fn a_logos_source_is_no_placements() {
    let folder = Folder::new("logo-source");
    let (mut s, sheet) = with_a_block();
    logo(&mut s, &folder, &red_blue(), "legal_owner");
    let placement = place(&mut s, &folder, &red_blue(), sheet, [10.0, 20.0, 60.0, 45.0]);
    let mut d = (**s.drawing("test").unwrap()).clone();
    let placed = d.sheet_images.by_id(placement).unwrap().clone();
    d.sheet_images.add([Arc::new(solidcraft_doc::drawing::image::SheetImage { id: 1, ..placed })]);
    let items = logo_items(&d, sheet);
    let [Item::Picture(picture)] = items.as_slice() else { panic!("{items:?}") };
    assert_eq!(picture.source, 2);
}

/// A logo's file that won't decode refuses the sheet, naming the file, and the provider counts a logo
/// toward what a sheet may hold decoded.
#[test]
fn a_logo_that_wont_decode_refuses_its_sheet() {
    let folder = Folder::new("logo-wont-decode");
    let (mut s, sheet) = with_a_block();
    let file = logo(&mut s, &folder, &png(2, 1), "legal_owner");
    let path = folder.path("sheet.svg");
    let e = s.execute("drawing.export", &json!({"sheet": ids::encode(sheet), "path": path})).unwrap_err().to_string();
    assert!(e.contains(&ids::encode(file)), "{e}");
    assert!(!std::path::Path::new(&path).exists());

    let big = logo(&mut s, &folder, &png(8000, 8000), "title");
    let d = s.drawing("test").unwrap().clone();
    assert!(d.images.by_id(big).is_some());
    let e = Provider::new(&d, sheet).err().unwrap();
    assert!(e.contains("pixels between them"), "{e}");
}

/// The provider's scene and the one built at once hold the same block, logo and all, and a logo set
/// since the last answer isn't missed.
#[test]
fn the_scene_holds_a_logo_with_or_without_its_cache() {
    let folder = Folder::new("logo-cache");
    let (mut s, sheet) = with_a_block();
    let before = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert!(!block_group(&before.scene).items.iter().any(|item| matches!(item, Item::Picture(_))));
    logo(&mut s, &folder, &red_blue(), "legal_owner");
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(&s, &d, sheet).unwrap();
    let cached = crate::drawing_scene::extract(&mut s, sheet).unwrap();
    assert_eq!(block_group(&cached.scene), block_group(&built.scene));
    assert!(matches!(block_group(&cached.scene).items[0], Item::Picture(_)));
}
