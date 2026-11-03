use std::path::PathBuf;

use serde_json::json;
use solidcraft_doc::canvas::base64_decode;
use solidcraft_geom::paper::MAX_IMAGE_PIXELS;
use solidcraft_io::vfs::{self, PathScope};

use super::*;
use crate::drawing_pictures::tests::png;
use crate::schema::tests::conforms;

const RECT: [f64; 4] = [10.0, 20.0, 60.0, 45.0];

/// A folder of this test's own, holding `bytes` as `logo.png`: the folder and the file.
fn on_disk(test: &str, bytes: &[u8]) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-drawing-image-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("logo.png");
    std::fs::write(&file, bytes).unwrap();
    (dir, file.to_string_lossy().into_owned())
}

/// A session with a box and a drawing of it, open, on the sheet it returns.
fn drawing() -> (Session, String) {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 80, "width": 40, "height": 30})).unwrap();
    s.execute("drawing.create", &json!({})).unwrap();
    let sheet = ids::encode(s.drawing("test").unwrap().sheets[0].id);
    (s, sheet)
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(ADD, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

#[test]
fn an_image_is_read_saved_without_its_path_and_one_undo_step() {
    let bytes = png(640, 480);
    let (dir, file) = on_disk("saved", &bytes);
    let (mut s, sheet) = drawing();
    let reply = s.execute(ADD, &json!({"sheet": sheet, "file": file, "rect_mm": RECT})).unwrap();
    let schema = output_schema_for::<AddResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!((&reply["format"], &reply["pixels"]), (&json!("png"), &json!([640, 480])), "{reply}");

    // The original file is gone: the design has its bytes, and doesn't know where they came from.
    std::fs::remove_dir_all(&dir).unwrap();
    let record = s.drawing("test").unwrap().clone();
    let id = ids::decode(reply["image"].as_str().unwrap()).unwrap();
    assert_eq!(base64_decode(&record.images.by_id(record.sheet_images.by_id(id).unwrap().image).unwrap().data).unwrap(), bytes);
    assert!(!record.to_json().unwrap().contains("logo"));

    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.drawing("test").unwrap().images.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(s.drawing("test").unwrap().sheet_images.by_id(id).is_some());
}

/// The same logo on two sheets is one file, and it goes when the last of them goes. Other bytes are
/// another file. A sheet's delete takes its placements, and the file stays for the other sheet.
#[test]
fn the_same_bytes_are_one_file_on_each_sheet() {
    let (dir, file) = on_disk("logo", &png(32, 32));
    let (other_dir, other) = on_disk("logo-other", &png(64, 64));
    let (mut s, first) = drawing();
    s.execute("drawing.sheet.add", &json!({})).unwrap();
    let second = ids::encode(s.drawing("test").unwrap().sheets[1].id);
    let add = |s: &mut Session, sheet: &str, file: &str| s.execute(ADD, &json!({"sheet": sheet, "file": file, "rect_mm": RECT})).unwrap();
    let a = add(&mut s, &first, &file);
    let b = add(&mut s, &second, &file);
    assert_ne!(a["image"], b["image"], "each place has its own id");
    assert_eq!(b["pixels"], json!([32, 32]));
    let d = s.drawing("test").unwrap().clone();
    assert_eq!((d.images.len(), d.sheet_images.len()), (1, 2), "stored once");
    assert_eq!(d.sheet_images[0].image, d.sheet_images[1].image);
    add(&mut s, &first, &other);
    assert_eq!(s.drawing("test").unwrap().images.len(), 2, "other bytes are another file");

    let why = s.execute("drawing.sheet.delete", &json!({"sheet": first})).unwrap_err().to_string();
    assert!(why.contains("2 images"), "a sheet with images on it says so: {why}");
    s.execute("drawing.sheet.delete", &json!({"sheet": first, "contents": "delete"})).unwrap();
    let left = s.drawing("test").unwrap();
    assert_eq!((left.images.len(), left.sheet_images.len()), (1, 1), "the other sheet still uses one file");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.drawing("test").unwrap().images.len(), 2, "an undo brings it back");
    s.execute("edit.redo", &json!({})).unwrap();
    let again = s.drawing("test").unwrap().to_json().unwrap();
    assert!(solidcraft_doc::drawing::Drawing::from_json(&again).is_ok());
    for d in [dir, other_dir] {
        std::fs::remove_dir_all(d).unwrap();
    }
}

#[test]
fn what_isnt_an_image_or_breaks_a_cap_is_refused_and_changes_nothing() {
    let (dir, bomb) = on_disk("refused", &png(100_000, 100_000));
    let (mut s, sheet) = drawing();
    let before = s.drawing("test").unwrap().clone();
    let side = (MAX_IMAGE_PIXELS as f64).sqrt() as u32 + 1;
    let text = dir.join("words.png");
    std::fs::write(&text, b"not a picture").unwrap();
    let big = dir.join("big.png");
    std::fs::write(&big, vec![0; MAX_IMAGE_BYTES + 1]).unwrap();
    let over = dir.join("over.png");
    std::fs::write(&over, png(side, side)).unwrap();
    let path = |p: &PathBuf| p.to_string_lossy().into_owned();
    let dir_text = path(&dir);
    let missing = path(&dir.join("missing.png"));
    let files: [&str; 10] = [&bomb, &path(&text), &path(&big), &path(&over), &dir_text, &missing, "", " ", "a\0b", &"x".repeat(4096)];
    for file in files {
        let why = refused(&mut s, json!({"sheet": sheet, "file": file, "rect_mm": RECT}));
        assert!(!why.is_empty(), "{file:?}");
    }
    for file in ["", " "] {
        let why = refused(&mut s, json!({"sheet": sheet, "file": file, "rect_mm": RECT}));
        assert!(why.contains("path of a PNG or JPEG"), "{file:?} names no file to look for: {why}");
    }
    let (good_dir, good) = on_disk("refused-good", &png(4, 4));
    assert!(refused(&mut s, json!({"sheet": "no such sheet", "file": good, "rect_mm": RECT})).contains("sheet"));
    for rect in [[60, 20, 10, 45], [10, 20, 10, 45], [0, 0, 1_000_000_000, 1]] {
        assert!(refused(&mut s, json!({"sheet": sheet, "file": good, "rect_mm": rect})).contains("rectangle"), "{rect:?}");
    }
    assert_eq!(**s.drawing("test").unwrap(), *before);
    for d in [dir, good_dir] {
        std::fs::remove_dir_all(d).unwrap();
    }
}

/// A file outside the folders an agent may read is refused by the same check every other read takes.
#[test]
fn a_file_outside_the_allowed_folders_is_refused() {
    let (dir, file) = on_disk("scope", &png(4, 4));
    let (allowed, _) = on_disk("scope-allowed", &png(4, 4));
    let (mut s, sheet) = drawing();
    let p = json!({"sheet": sheet, "file": file, "rect_mm": RECT});
    let answer = vfs::scoped(Some(PathScope::new([allowed.clone()])), || s.execute(ADD, &p));
    assert!(answer.is_err(), "{answer:?}");
    assert!(s.drawing("test").unwrap().images.is_empty());
    for d in [dir, allowed] {
        std::fs::remove_dir_all(d).unwrap();
    }
}

/// Under the images' own cap, with another tool's properties filling the rest of the record: the
/// save's limit is what refuses the image, and the drawing as it stands still saves.
#[test]
fn an_image_that_would_stop_the_drawing_saving_is_refused() {
    let sized = |w: u32| {
        let mut file = png(w, 4);
        file.resize(MAX_IMAGE_BYTES, 0);
        file
    };
    let (dir, file) = on_disk("save-a", &sized(4));
    let (dir_b, file_b) = on_disk("save-b", &sized(5));
    let (mut s, sheet) = drawing();
    s.edit_drawing("test", |d| {
        Arc::make_mut(&mut d.properties).0.insert("com.example".into(), json!({"text": "x".repeat(6 << 20)}));
        Ok(())
    })
    .unwrap();
    let p = |file: &str| json!({"sheet": sheet, "file": file, "rect_mm": RECT});
    s.execute(ADD, &p(&file)).unwrap();
    assert!(refused(&mut s, p(&file_b)).contains("too large to save"));
    s.drawing("test").unwrap().to_json().unwrap();
    for d in [dir, dir_b] {
        std::fs::remove_dir_all(d).unwrap();
    }
}

/// A drawing that couldn't be saved again is refused when the image is added, not at the save.
#[test]
fn an_image_a_drawing_has_no_room_for_is_refused() {
    let mut full = png(4, 4);
    full.resize(MAX_IMAGE_BYTES, 0);
    let (dir, file) = on_disk("room", &full);
    let (mut s, sheet) = drawing();
    let (dir_b, file_b) = on_disk("room-b", &{
        let mut b = png(5, 4);
        b.resize(MAX_IMAGE_BYTES, 0);
        b
    });
    let (dir_c, file_c) = on_disk("room-c", &{
        let mut c = png(6, 4);
        c.resize(MAX_IMAGE_BYTES / 2, 0);
        c
    });
    let p = |file: &str| json!({"sheet": sheet, "file": file, "rect_mm": RECT});
    s.execute(ADD, &p(&file)).unwrap();
    s.execute(ADD, &p(&file_b)).unwrap();
    s.execute(ADD, &p(&file)).unwrap();
    assert!(refused(&mut s, p(&file_c)).contains("between them"), "the same file again costs nothing; a third doesn't fit");
    s.drawing("test").unwrap().to_json().unwrap();
    for d in [dir_b, dir_c] {
        std::fs::remove_dir_all(d).unwrap();
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
