use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};
use solidcraft_geom::title_block::CellKind;

use super::*;
use crate::ids::IdSource;

const RECT: [f64; 4] = [10.0, 20.0, 60.0, 45.0];

/// A PNG's header, saying these pixels: all a cap reads.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0]);
    b
}

/// A JPEG's start, saying these pixels.
fn jpeg(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF, 0xC0, 0, 17, 8];
    b.extend(h.to_be_bytes());
    b.extend(w.to_be_bytes());
    b.extend([0; 12]);
    b
}

/// A drawing on one A3 sheet with one image of `bytes` placed on it: the drawing, the file's id and
/// the placement's.
fn pictured(bytes: &[u8]) -> (Drawing, u64, u64) {
    let mut id_source = IdSource::seeded(5426);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Pictured", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let (file, placed) = (d.allocate_id(&mut id_source).unwrap(), d.allocate_id(&mut id_source).unwrap());
    d.images.add([Arc::new(ImageFile::from_file(file, bytes).unwrap())]);
    d.sheet_images.add([Arc::new(SheetImage { id: placed, sheet: d.sheets[0].id, rect_mm: RECT, image: file })]);
    d.validate().unwrap();
    (d, file, placed)
}

/// A copy of the drawing's first sheet, as a sheet of its own.
fn drawing_sheet_copy(d: &Drawing, id: u64) -> crate::drawing::Sheet {
    crate::drawing::Sheet { id, name: "Sheet 2".into(), ..(*d.sheets[0]).clone() }
}

fn file(d: &Drawing) -> Value {
    serde_json::from_str(&d.to_json().unwrap()).unwrap()
}

fn refused(d: &Drawing, edit: impl FnOnce(&mut Value)) -> String {
    let mut v = file(d);
    edit(&mut v);
    Drawing::from_json(&v.to_string()).map_or_else(|e| e.to_string(), |_| "read".into())
}

#[test]
fn an_image_is_saved_with_its_bytes_and_reads_back_the_same() {
    let bytes = png(640, 480);
    let (d, image, placed) = pictured(&bytes);
    let saved = d.to_json().unwrap();
    let read = Drawing::from_json(&saved).unwrap();
    assert_eq!(read, d);
    assert_eq!(read.to_json().unwrap(), saved);
    let found = read.images.by_id(image).unwrap();
    assert_eq!((found.format.as_str(), found.pixels), ("png", [640, 480]));
    assert_eq!(base64_decode(&found.data).unwrap(), bytes);
    assert_eq!(read.sheet_images.by_id(placed).map(|i| (i.rect_mm, i.image)), Some((RECT, image)));
    assert!(read.images.by_id(placed).is_none() && read.sheet_images.by_id(image).is_none());
}

/// One file sits on two sheets and is saved once. Its bytes count once towards the caps, so a logo on
/// every sheet costs what one does.
#[test]
fn one_file_on_two_sheets_is_saved_once() {
    let mut logo = png(10, 10);
    logo.resize(1 << 20, 0);
    let (mut d, image, _) = pictured(&logo);
    let first = d.sheets[0].id;
    let mut id_source = IdSource::seeded(4);
    let second = d.allocate_id(&mut id_source).unwrap();
    let sheet = drawing_sheet_copy(&d, second);
    d.sheets.push(Arc::new(sheet));
    for n in 0..20u64 {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.sheet_images.add([Arc::new(SheetImage { id, sheet: second, rect_mm: [n as f64, 0.0, n as f64 + 5.0, 5.0], image })]);
    }
    d.validate().unwrap();
    assert_eq!((d.images.len(), d.sheet_images.len()), (1, 21));
    let read = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(read, d);
    assert!(d.to_json().unwrap().len() < 2 << 20, "the bytes are there once");
    assert_eq!(d.images_in_use(), BTreeSet::from([image]));

    // The first sheet's going leaves the file for the second's, and the second's takes it.
    d.remove_sheet(first);
    assert_eq!((d.images.len(), d.sheet_images.len()), (1, 20));
    d.validate().unwrap();
    d.remove_sheet(second);
    assert!(d.images.is_empty() && d.sheet_images.is_empty());
}

/// A file goes with its last placement, and nothing is left to save.
#[test]
fn a_file_goes_with_its_last_placement() {
    let (mut d, _, placed) = pictured(&png(1, 1));
    let (sheet, other) = (d.sheets[0].id, d.allocate_id(&mut IdSource::seeded(4)).unwrap());
    let empty = drawing_sheet_copy(&d, other);
    d.sheets.push(Arc::new(empty));
    assert_eq!(d.on_sheet(sheet).images, [placed]);
    assert!(!d.on_sheet(sheet).is_empty() && d.on_sheet(other).is_empty());
    assert_eq!(d.remove_sheet(sheet).images, [placed]);
    assert!(d.images.is_empty() && d.sheet_images.is_empty());
    d.validate().unwrap();
    let v = file(&d);
    assert!(v.get("images").is_none() && v.get("sheet_images").is_none(), "{v}");
    assert_eq!(Drawing::from_json(&v.to_string()).unwrap(), d, "an older drawing has neither");
}

#[test]
fn ids_stay_apart_across_files_and_placements() {
    let (d, image, placed) = pictured(&png(1, 1));
    let taken = d.ids().collect::<BTreeSet<_>>();
    assert!(taken.contains(&image) && taken.contains(&placed));
    let mut twin = d.clone();
    Arc::make_mut(&mut twin.sheet_images[0]).id = image;
    assert!(twin.validate().is_err_and(|e| e.to_string().contains("two objects")));
}

#[test]
fn the_caps_are_read_from_the_file_before_anything_decodes_it() {
    let make = |b: &[u8]| ImageFile::from_file(1, b).map(|i| i.pixels).map_err(|e| e.to_string());
    assert_eq!(make(&png(8_000, 8_000)), Ok([8_000, 8_000]));
    assert_eq!(make(&jpeg(8_000, 8_000)), Ok([8_000, 8_000]));
    // The limits, one over, and sizes whose product would overflow 32 bits.
    for (w, h) in [(8_001, 8_000), (64_000_001, 1), (1, 64_000_001), (65_536, 65_536), (u32::MAX, u32::MAX)] {
        assert!(make(&png(w, h)).is_err_and(|e| e.contains("pixels")), "{w} by {h}");
    }
    assert!(make(&jpeg(u16::MAX, u16::MAX)).is_err_and(|e| e.contains("pixels")));
    let not_images: [&[u8]; 6] = [b"", b"GIF89a", b"\x89PNG", &png(0, 5), &png(5, 0), &jpeg(0, 5)];
    for not_image in not_images {
        assert!(make(not_image).is_err_and(|e| e.contains("PNG or JPEG")), "{not_image:?}");
    }
    let mut full = png(10, 10);
    full.resize(MAX_IMAGE_BYTES, 0);
    assert_eq!(make(&full), Ok([10, 10]));
    full.push(0);
    assert!(make(&full).is_err_and(|e| e.contains("MiB")));
}

#[test]
fn a_file_that_lies_is_refused_on_reading() {
    let (d, ..) = pictured(&png(10, 10));
    let with = |key: &'static str, value: Value| move |v: &mut Value| v["images"][0][key] = value;
    assert!(refused(&d, with("pixels", json!([1, 1]))).contains("disagree"));
    assert!(refused(&d, with("format", "jpeg".into())).contains("disagree"));
    assert!(refused(&d, with("format", "gif".into())).contains("PNG or a JPEG"));
    assert!(refused(&d, with("data", "!!!".into())).contains("base64"));
    assert!(refused(&d, with("data", base64_encode(&png(9_000, 9_000)).into())).contains("pixels"));
    assert!(refused(&d, with("data", "".into())).contains("PNG or JPEG"));
    assert!(refused(&d, with("pixels", json!([0, 10]))).contains("pixels"));
    assert!(refused(&d, with("pixels", json!([65_536, 65_536]))).contains("pixels"));
    assert!(refused(&d, with("path", "C:/secret.png".into())).contains("path"), "an unknown field is damage");
}

/// A placement names a file and a sheet the drawing has, and a file nothing places isn't kept.
#[test]
fn a_placement_and_its_file_name_each_other() {
    let (d, ..) = pictured(&png(10, 10));
    let placed = |key: &'static str, value: Value| move |v: &mut Value| v["sheet_images"][0][key] = value;
    assert!(refused(&d, placed("sheet", ids::encode(1 << 41).into())).contains("sheet"));
    assert!(refused(&d, placed("image", ids::encode(1 << 41).into())).contains("doesn't have"));
    assert!(refused(&d, |v| v["sheet_images"] = json!([])).contains("nothing uses it"));
    assert!(refused(&d, |v| v["images"] = json!([])).contains("doesn't have"));
    assert!(refused(&d, placed("data", "AAAA".into())).contains("data"), "the file's bytes aren't the placement's");
}

#[test]
fn a_rectangle_is_on_paper_and_has_a_size() {
    let (d, ..) = pictured(&png(10, 10));
    for rect in [
        [60.0, 20.0, 10.0, 45.0],
        [10.0, 45.0, 60.0, 20.0],
        [10.0, 20.0, 10.0, 45.0],
        [10.0, 20.0, 60.0, 20.0],
        [1e300, 20.0, 2e300, 45.0],
        [f64::NAN; 4],
    ] {
        let mut bad = d.clone();
        Arc::make_mut(&mut bad.sheet_images[0]).rect_mm = rect;
        assert!(bad.validate().is_err_and(|e| e.to_string().contains("rectangle")), "{rect:?}");
    }
}

#[test]
fn a_drawings_images_keep_their_counts() {
    let (d, image, placed) = pictured(&png(1, 1));
    // A file and something to use it, ids after the first ones.
    let again = |d: &mut Drawing, n: u64, data: &str| {
        let mut file = (*d.images[0]).clone();
        file.id = image + 2 * n;
        file.data = data.into();
        let use_of = SheetImage { id: placed + 2 * n, sheet: d.sheets[0].id, rect_mm: RECT, image: file.id };
        d.images.add([Arc::new(file)]);
        d.sheet_images.add([Arc::new(use_of)]);
    };
    // One file's cap and the cap between them, checked on every edit from the text's length alone.
    let mut big = d.clone();
    Arc::make_mut(&mut big.images[0]).data = "A".repeat(encoded(MAX_IMAGE_BYTES) + 1);
    assert!(big.validate().is_err_and(|e| e.to_string().contains("its file")));
    let mut many = d.clone();
    let full = "A".repeat(encoded(MAX_IMAGE_BYTES));
    Arc::make_mut(&mut many.images[0]).data.clone_from(&full);
    again(&mut many, 1, &full);
    let room = encoded(MAX_IMAGES_BYTES) - 2 * full.len();
    again(&mut many, 2, &"A".repeat(room));
    many.validate().unwrap();
    again(&mut many, 3, "A");
    assert!(many.validate().is_err_and(|e| e.to_string().contains("between them")));
    let mut crowd = d.clone();
    (1..=MAX_IMAGES as u64).for_each(|n| again(&mut crowd, n, "AAAA"));
    assert!(crowd.validate().is_err_and(|e| e.to_string().contains("at most")));
    let mut placements = d.clone();
    let logo = placements.sheet_images[0].clone();
    let copies = (1..=MAX_SHEET_IMAGES as u64).map(|n| Arc::new(SheetImage { id: placed + 2 * n, ..(*logo).clone() }));
    placements.sheet_images.add(copies.collect::<Vec<_>>());
    assert!(placements.validate().is_err_and(|e| e.to_string().contains("at most")));
    let mut twins = d.clone();
    let twin = twins.sheet_images[0].clone();
    twins.sheet_images.push(twin);
    assert!(twins.validate().is_err());
}

/// A drawing of two sheets with the built-in block, on A3.
fn with_blocks() -> (Drawing, IdSource) {
    let mut id_source = IdSource::seeded(3875);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d =
        Drawing::new("Logos", "en", "Sheet 1", a3, Some(drawing_sheet::DrawingBorderProfile::iso()), &BTreeSet::new(), &mut id_source).unwrap();
    Arc::make_mut(&mut d.sheets[0]).title_block = Some(solidcraft_geom::title_block::default_block());
    let second = d.allocate_id(&mut id_source).unwrap();
    d.sheets.push(Arc::new(drawing_sheet_copy(&d, second)));
    d.validate().unwrap();
    (d, id_source)
}

fn logo_of(d: &Drawing, sheet: usize) -> Option<u64> {
    d.sheets[sheet].title_block.as_ref().unwrap().logos().next()
}

/// A logo on every sheet is one file, the same bytes again reuse it, it comes back the same after a
/// save, and it goes with its last use; a placement of the same file keeps it.
#[test]
fn a_logo_is_a_use_of_its_file() {
    let (mut d, mut id_source) = with_blocks();
    let bytes = png(32, 32);
    let new_file = |d: &Drawing, id_source: &mut IdSource| ImageFile::from_file(d.allocate_id(id_source).unwrap(), &bytes).unwrap();
    let file = new_file(&d, &mut id_source);
    let (sheets, shown) = d.set_logo(None, "legal_owner", Some(file)).unwrap();
    let id = logo_of(&d, 0).unwrap();
    assert_eq!((sheets.len(), shown), (2, Some(id)));
    assert_eq!((logo_of(&d, 1), d.images.iter().count(), d.images_in_use()), (Some(id), 1, BTreeSet::from([id])));
    d.validate().unwrap();
    let again = new_file(&d, &mut id_source);
    d.set_logo(None, "title", Some(again)).unwrap();
    assert_eq!(d.images.iter().count(), 1, "the same bytes are one file");

    let back = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!((logo_of(&back, 0), base64_decode(&back.images.by_id(id).unwrap().data).unwrap()), (Some(id), bytes.clone()));

    d.set_logo(None, "title", None).unwrap();
    let first = d.sheets[0].id;
    d.set_logo(Some(&[first]), "legal_owner", None).unwrap();
    assert_eq!((logo_of(&d, 0), logo_of(&d, 1), d.images.iter().count()), (None, Some(id), 1), "the other sheet still shows it");
    d.set_logo(None, "legal_owner", None).unwrap();
    assert!(d.images.is_empty(), "it goes with its last use");
    d.validate().unwrap();
}

#[test]
fn a_logo_goes_only_in_a_text_cell_of_a_block() {
    let (mut d, mut id_source) = with_blocks();
    let file = ImageFile::from_file(d.allocate_id(&mut id_source).unwrap(), &png(8, 8)).unwrap();
    let was = d.clone();
    let fails = |d: &mut Drawing, sheets: Option<&[u64]>, cell: &str| d.set_logo(sheets, cell, Some(file.clone())).unwrap_err().to_string();
    assert!(fails(&mut d, None, "projection").contains("isn't a text cell"));
    d = was.clone();
    assert!(fails(&mut d, None, "nothing").contains("no sheet's title block"));
    d = was.clone();
    let first = d.sheets[0].id;
    assert!(fails(&mut d, Some(&[first]), "nothing").contains("no title block cell"));
    d = was.clone();
    assert!(fails(&mut d, Some(&[1]), "title").contains("no sheet"));
}

/// A link to a file the drawing doesn't have is refused on read, and so is a file only an emptied
/// cell used to show.
#[test]
fn a_damaged_logo_link_is_refused_on_read() {
    let (mut d, mut id_source) = with_blocks();
    let id = d.allocate_id(&mut id_source).unwrap();
    d.set_logo(None, "title", Some(ImageFile::from_file(id, &png(8, 8)).unwrap())).unwrap();
    let cell = "/sheets/0/title_block/cells/0/kind/text/image";
    let mut lost: Value = file(&d);
    *lost.pointer_mut(cell).unwrap() = json!(crate::ids::encode(id + 1));
    assert!(Drawing::from_value(lost).unwrap_err().to_string().contains("which the drawing doesn't have"));
    let mut unused: Value = file(&d);
    for sheet in 0..2 {
        unused.pointer_mut(&format!("/sheets/{sheet}/title_block/cells/0/kind/text")).unwrap().as_object_mut().unwrap().remove("image");
    }
    assert!(Drawing::from_value(unused).unwrap_err().to_string().contains("nothing uses it"));
}

/// A template over a block with a logo takes the logo with the old block.
#[test]
fn a_logo_goes_with_the_block_a_template_replaces() {
    let (mut d, mut id_source) = with_blocks();
    let id = d.allocate_id(&mut id_source).unwrap();
    d.set_logo(None, "title", Some(ImageFile::from_file(id, &png(8, 8)).unwrap())).unwrap();
    let chosen = crate::drawing::template::built_in("iso-a3-part").unwrap().unwrap();
    let (applied, _) = crate::drawing::template::apply(&d, &chosen, &mut id_source).unwrap();
    assert!(applied.images.is_empty() && logo_of(&applied, 0).is_none());
}

/// A template saved from a drawing keeps its logo's file and leaves placed images out. A drawing made
/// from it shows the logo; applied to a drawing whose own file has that id, it brings its file in under
/// another id, and applied again changes nothing.
#[test]
fn a_template_carries_its_logos_file() {
    use crate::drawing::template;

    let (mut d, mut id_source) = with_blocks();
    let logo = png(16, 16);
    let id = d.allocate_id(&mut id_source).unwrap();
    d.set_logo(None, "legal_owner", Some(ImageFile::from_file(id, &logo).unwrap())).unwrap();
    let placed = d.allocate_id(&mut id_source).unwrap();
    d.sheet_images.add([Arc::new(SheetImage { id: placed, sheet: d.sheets[0].id, rect_mm: RECT, image: id })]);
    let saved = template::from_drawing(&d).unwrap();
    assert!(saved.sheet_images.is_empty());
    assert_eq!(saved.images.iter().count(), 1);
    let read = template::read(&saved.to_json().unwrap()).unwrap();
    let made = template::instantiate(&read, "From a template", &BTreeSet::new(), &mut id_source).unwrap();
    assert_eq!(base64_decode(&made.images.by_id(logo_of(&made, 0).unwrap()).unwrap().data).unwrap(), logo);

    // The drawing has another file under the template's file's id.
    let (mut other, _, _) = pictured(&png(99, 99));
    let (taken, placement) = (other.images.iter().next().unwrap().id, other.sheet_images.iter().next().unwrap().id);
    let mut seeded = IdSource::seeded(1);
    Arc::make_mut(&mut other.sheets[0]).title_block = Some(solidcraft_geom::title_block::default_block());
    let mut clash = read.clone();
    let file = ImageFile { id: taken, ..(**clash.images.iter().next().unwrap()).clone() };
    clash.images = Default::default();
    clash.images.add([Arc::new(file)]);
    for sheet in &mut clash.sheets {
        Arc::make_mut(sheet).title_block.as_mut().unwrap().cells.iter_mut().for_each(|cell| {
            if let CellKind::Text(text) = &mut cell.kind {
                text.image = text.image.map(|_| taken);
            }
        });
    }
    template::check(&clash).unwrap();
    let (applied, changed) = template::apply(&other, &clash, &mut seeded).unwrap();
    assert_eq!(changed.len(), 1);
    let shown = logo_of(&applied, 0).unwrap();
    assert_ne!(shown, taken);
    assert_eq!(base64_decode(&applied.images.by_id(shown).unwrap().data).unwrap(), logo);
    assert_eq!(base64_decode(&applied.images.by_id(taken).unwrap().data).unwrap(), png(99, 99), "the placed image keeps its own");
    assert!(applied.sheet_images.by_id(placement).is_some());
    let (again, changed) = template::apply(&applied, &clash, &mut seeded).unwrap();
    assert_eq!((changed.len(), again.images.iter().count()), (0, 2), "the same bytes are the same file");

    let mut placed_in_it = read;
    placed_in_it.sheet_images.add([Arc::new(SheetImage { id: placed, sheet: placed_in_it.sheets[0].id, rect_mm: RECT, image: id })]);
    assert!(template::check(&placed_in_it).unwrap_err().to_string().contains("images placed"));
}
