#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn refused(s: &mut Session, command: &str, params: Value) -> String {
    s.execute(command, &params).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{command} should refuse"))
}

/// A fresh folder of its own for each test, removed afterwards.
struct Folder(PathBuf);

impl Folder {
    fn new(case: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-copy-links-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }
    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn part(path: &Path, height: u32) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": height, "corner": [0, 0, 0]}));
    run(&mut s, "file.save", json!({"path": path}));
}

fn linking(path: &Path, parts: &[&Path]) {
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": path}));
    for (k, p) in parts.iter().enumerate() {
        run(&mut s, "component.insert_design", json!({"path": p, "translate": [40.0 * k as f64, 0, 0]}));
    }
    run(&mut s, "file.save", json!({"path": path}));
}

fn drawing_of(path: &Path, source: &Path) {
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "drawing.create", json!({"source": source}));
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "file.save", json!({"path": path}));
}

/// Each link of the design at `path` as `component.links` reports it: its file and whether that
/// changed since the design last read it.
fn links(path: &Path) -> Vec<(String, Option<bool>)> {
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path}));
    let listed = run(&mut s, "component.links", json!({}));
    let mut out: Vec<(String, Option<bool>)> =
        listed["links"].as_array().unwrap().iter().map(|l| (l["file"].as_str().unwrap().to_string(), l["changed"].as_bool())).collect();
    out.sort();
    out
}

fn drawn(path: &Path) -> Vec<String> {
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path}));
    s.doc.drawings.iter().flat_map(|d| d.sources.iter().filter_map(|s| s.path.clone())).collect()
}

/// A skid linking a plate and a frame (which links a pin), a bolt from a library folder elsewhere,
/// a drawing of the plate beside them, and a design beside them that links none of them.
struct Skid {
    folder: Folder,
    skid: PathBuf,
    plate: PathBuf,
    frame: PathBuf,
    pin: PathBuf,
    bolt: PathBuf,
    drawing: PathBuf,
}

fn skid(case: &str) -> Skid {
    let folder = Folder::new(case);
    let (src, lib) = (folder.dir("skid"), folder.dir("library"));
    let (plate, pin, bolt) = (src.join("plate.sc"), src.join("pin.sc"), lib.join("bolt.sc"));
    part(&plate, 5);
    part(&pin, 30);
    part(&bolt, 12);
    let frame = src.join("frame.sc");
    linking(&frame, &[&pin]);
    let skid = src.join("skid.sc");
    linking(&skid, &[&plate, &frame, &bolt]);
    let drawing = src.join("plate drawing.sc");
    drawing_of(&drawing, &plate);
    part(&src.join("unrelated.sc"), 1);
    Skid { folder, skid, plate, frame, pin, bolt, drawing }
}

#[test]
fn a_skid_copies_with_its_parts_and_drawing_linked_to_the_copies_and_its_library_part_left_in_place() {
    let k = skid("whole");
    let mut s = Session::default();
    let listed = run(&mut s, "file.copy_with_links.list", json!({"path": k.skid}));
    let items = listed["items"].as_array().unwrap();
    let kind = |p: &Path| items.iter().find(|i| i["path"] == text(p)).map(|i| i["kind"].as_str().unwrap().to_string());
    assert_eq!(items[0]["path"], text(&k.skid), "{listed}");
    assert_eq!(items.len(), 6, "skid, plate, frame, pin, bolt and the plate's drawing: {listed}");
    for p in [&k.skid, &k.plate, &k.frame, &k.pin, &k.bolt] {
        assert_eq!(kind(p).as_deref(), Some("design"), "{p:?} in {listed}");
    }
    assert_eq!(kind(&k.drawing).as_deref(), Some("drawing"), "{listed}");
    let drawing = items.iter().find(|i| i["path"] == text(&k.drawing)).unwrap();
    assert_eq!(drawing["used_by"], json!([text(&k.plate)]), "a drawing is listed as used by what it draws");
    let pin = items.iter().find(|i| i["path"] == text(&k.pin)).unwrap();
    assert_eq!(pin["used_by"], json!([text(&k.frame)]));

    let originals: Vec<Vec<u8>> = [&k.skid, &k.plate, &k.frame, &k.pin, &k.bolt, &k.drawing].iter().map(|p| std::fs::read(p).unwrap()).collect();
    let out = k.folder.dir("skid 2");
    let copy = |p: &Path, name: &str| json!({"path": p, "name": name});
    let made = run(
        &mut s,
        "file.copy_with_links",
        json!({"path": k.skid, "folder": out, "copy": [
            copy(&k.skid, "skid-2.sc"), copy(&k.plate, "plate-2.sc"), copy(&k.frame, "frame-2.sc"),
            copy(&k.pin, "pin-2.sc"), copy(&k.drawing, "plate-2 drawing.sc"),
        ]}),
    );
    assert_eq!(made["design"], text(&out.join("skid-2.sc")), "{made}");
    assert_eq!(made["copied"].as_array().unwrap().len(), 5, "{made}");
    assert_eq!(made["kept"], json!([text(&k.bolt)]), "{made}");

    // The copies link each other; the bolt is still the library's; every link is up to date.
    let up_to_date = |files: &[&Path]| {
        let mut want: Vec<(String, Option<bool>)> = files.iter().map(|p| (text(p), Some(false))).collect();
        want.sort();
        want
    };
    assert_eq!(links(&out.join("skid-2.sc")), up_to_date(&[&out.join("plate-2.sc"), &out.join("frame-2.sc"), &k.bolt]));
    assert_eq!(links(&out.join("frame-2.sc")), up_to_date(&[&out.join("pin-2.sc")]));
    // A drawing keeps its source relative to its own file, as saving does.
    assert_eq!(drawn(&out.join("plate-2 drawing.sc")), vec![text(&out.join("plate-2.sc"))]);
    // The originals are as they were.
    for (p, before) in [&k.skid, &k.plate, &k.frame, &k.pin, &k.bolt, &k.drawing].iter().zip(&originals) {
        assert_eq!(&std::fs::read(p).unwrap(), before, "{p:?} changed");
    }
    assert!(!out.join("bolt.sc").exists());
}

#[test]
fn a_design_left_out_stays_linked_where_it_is_from_the_copies() {
    let k = skid("left-out");
    let out = k.folder.dir("out");
    let mut s = Session::default();
    run(
        &mut s,
        "file.copy_with_links",
        json!({"path": k.skid, "drawings": false, "folder": out, "copy": [{"path": k.skid, "name": "skid-3.sc"}, {"path": k.plate, "name": "plate-3.sc"}]}),
    );
    let mut want = vec![(text(&out.join("plate-3.sc")), Some(false)), (text(&k.frame), Some(false)), (text(&k.bolt), Some(false))];
    want.sort();
    assert_eq!(links(&out.join("skid-3.sc")), want);
    let names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn copying_refuses_a_bad_pick_and_writes_nothing() {
    let k = skid("refusals");
    let out = k.folder.dir("out");
    let mut s = Session::default();
    let call = |copy: Value| json!({"path": k.skid, "folder": out, "copy": copy});
    let top = json!({"path": k.skid, "name": "skid-2.sc"});
    let cases = [
        (call(json!([{"path": k.plate, "name": "plate-2.sc"}])), "must be copied"),
        (call(json!([top, {"path": k.plate, "name": "sub/plate.sc"}])), "not a file name"),
        (call(json!([top, {"path": k.plate, "name": "plate.step"}])), "extension"),
        (call(json!([top, {"path": k.plate, "name": "SKID-2.sc"}])), "two copies"),
        (call(json!([top, {"path": k.folder.0.join("elsewhere.sc"), "name": "x.sc"}])), "not one of the design's files"),
        (json!({"path": k.skid, "folder": k.folder.0.join("no such folder"), "copy": [top]}), "not a folder"),
    ];
    for (params, why) in cases {
        let e = refused(&mut s, "file.copy_with_links", params.clone());
        assert!(e.contains(why), "{params}: {e}");
    }
    // A file already at a new name refuses the whole call, and it is left as it was.
    std::fs::write(out.join("plate-2.sc"), b"mine").unwrap();
    let e = refused(&mut s, "file.copy_with_links", call(json!([top, {"path": k.plate, "name": "plate-2.sc"}])));
    assert!(e.contains("already exists"), "{e}");
    assert_eq!(std::fs::read(out.join("plate-2.sc")).unwrap(), b"mine");
    let names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["plate-2.sc".to_string()]);
}

#[test]
fn the_open_design_is_copied_from_its_file_once_saved() {
    let k = skid("open");
    let mut s = Session::default();
    let e = refused(&mut s, "file.copy_with_links.list", json!({}));
    assert!(e.contains("Save the design first"), "{e}");
    run(&mut s, "doc.open", json!({"path": k.skid}));
    let listed = run(&mut s, "file.copy_with_links.list", json!({}));
    assert_eq!(listed["design"], text(&k.skid));
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [100, 0, 0]}));
    let e = refused(&mut s, "file.copy_with_links.list", json!({}));
    assert!(e.contains("Save the design's changes first"), "{e}");
}

#[test]
fn a_drawing_beside_a_part_in_another_folder_is_listed_and_copied_with_it() {
    // The bolt lives in a library folder of its own, with its drawing beside it; a design there that
    // draws nothing listed is not.
    let k = skid("other-folder");
    let lib = k.bolt.parent().unwrap().to_path_buf();
    let bolt_drawing = lib.join("bolt drawing.sc");
    drawing_of(&bolt_drawing, &k.bolt);
    part(&lib.join("washer.sc"), 2);
    let mut s = Session::default();
    let listed = run(&mut s, "file.copy_with_links.list", json!({"path": k.skid}));
    let items = listed["items"].as_array().unwrap();
    let drawing = items.iter().find(|i| i["path"] == text(&bolt_drawing)).unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(drawing["kind"], "drawing");
    assert_eq!(drawing["used_by"], json!([text(&k.bolt)]));
    assert!(!items.iter().any(|i| i["path"] == text(&lib.join("washer.sc"))), "{listed}");

    let out = k.folder.dir("out");
    let copy = |p: &Path, name: &str| json!({"path": p, "name": name});
    run(&mut s, "file.copy_with_links", json!({"path": k.skid, "folder": out, "copy": [
        copy(&k.skid, "skid.sc"), copy(&k.bolt, "bolt.sc"), copy(&bolt_drawing, "bolt drawing.sc"),
    ]}));
    assert_eq!(drawn(&out.join("bolt drawing.sc")), vec![text(&out.join("bolt.sc"))]);
}
