use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};

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
