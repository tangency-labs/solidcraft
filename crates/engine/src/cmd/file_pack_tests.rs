use std::io::Read;
use std::path::Path;

use serde_json::{Value, json};

use super::*;
use crate::schema::tests::conforms;
use crate::tests::Folder;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(CMD, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("packed: {p}"))
}

/// A box `height` mm high called Plate, saved at `path`.
fn save_box(path: &str, height: f64) {
    let mut part = Session::default();
    run(&mut part, "solid.box", json!({"length": 10, "width": 20, "height": height, "body_name": "Plate"}));
    run(&mut part, "file.save", json!({"path": path}));
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// A design saved at `path` linking each of `parts`.
fn assembly(path: &str, parts: &[&str]) -> Session {
    let mut s = Session::default();
    for part in parts {
        run(&mut s, "component.insert_design", json!({"path": part}));
    }
    run(&mut s, "file.save", json!({"path": path}));
    s
}

/// Each entry of the pack at `path`, by name, unpacked into `into`.
fn unpack(path: &str, into: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let to = into.join(&name);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(&to, &bytes).unwrap();
        entries.push((name, bytes));
    }
    entries
}

/// A pack holds the design at the top level and each design its links name once under
/// `designs/`, byte for byte; unpacked anywhere it opens with every link found and the same
/// model. The design, its file and its links are left as they were.
#[test]
fn a_pack_opens_anywhere_with_every_link_found() {
    let folder = Folder::new("pack-links");
    std::fs::create_dir_all(folder.0.join("parts")).unwrap();
    std::fs::create_dir_all(folder.0.join("asm")).unwrap();
    let plate = folder.path("parts/plate.solidcraft");
    save_box(&plate, 30.0);
    let case = folder.path("asm/case.solidcraft");
    // Two placements of one linked design: one file in the pack.
    let mut s = assembly(&case, &[&plate, &plate]);
    let (before, saved, doc) = (volume(&mut s), std::fs::read(&case).unwrap(), s.doc.clone());
    let zip = folder.path("out.zip");
    let reply = run(&mut s, CMD, json!({"path": zip}));
    let schema = output_schema_for::<PackResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(
        (&reply["design"], &reply["designs"][0]["entry"], &reply["deeper"]),
        (&json!("case.solidcraft"), &json!("designs/plate.solidcraft"), &json!([]))
    );
    assert_eq!(reply["designs"].as_array().map(Vec::len), Some(1), "{reply}");
    assert_eq!(reply["bytes"].as_u64(), Some(std::fs::metadata(&zip).unwrap().len()));
    let elsewhere = Folder::new("pack-links-unpacked");
    let entries = unpack(&zip, &elsewhere.0);
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["designs/plate.solidcraft", "case.solidcraft"]);
    assert_eq!(entries[0].1, std::fs::read(&plate).unwrap(), "byte for byte");
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": elsewhere.path("case.solidcraft")}));
    let links = run(&mut opened, "component.links", json!({}));
    for link in links["links"].as_array().unwrap() {
        assert_eq!(link["found"], true, "{links}");
        assert_eq!(super::absolute(Path::new(link["file"].as_str().unwrap())), super::absolute(&elsewhere.0.join("designs/plate.solidcraft")));
    }
    assert!((volume(&mut opened) - before).abs() < 1e-6);
    // Nothing here changed.
    assert_eq!(std::fs::read(&case).unwrap(), saved);
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc) && !s.is_dirty() && s.path.as_deref() == Some(case.as_str()));
}

/// Linked files of one name are numbered apart, and a linked design's own links are listed, not
/// followed: the pack goes one level deep.
#[test]
fn names_are_numbered_apart_and_deeper_links_listed() {
    let folder = Folder::new("pack-names");
    for dir in ["a", "b"] {
        std::fs::create_dir_all(folder.0.join(dir)).unwrap();
    }
    let (first, second, bolt) = (folder.path("a/plate.solidcraft"), folder.path("b/plate.solidcraft"), folder.path("b/bolt.solidcraft"));
    save_box(&first, 30.0);
    save_box(&bolt, 5.0);
    assembly(&second, &[&bolt]);
    let mut s = assembly(&folder.path("case.solidcraft"), &[&first, &second]);
    let reply = run(&mut s, CMD, json!({"path": folder.path("case.zip")}));
    let entries: Vec<&str> = reply["designs"].as_array().unwrap().iter().map(|d| d["entry"].as_str().unwrap()).collect();
    assert_eq!(entries, ["designs/plate.solidcraft", "designs/plate-2.solidcraft"]);
    assert_eq!(reply["deeper"], json!([{"design": "designs/plate-2.solidcraft", "path": "bolt.solidcraft"}]));
    let elsewhere = Folder::new("pack-names-unpacked");
    unpack(&folder.path("case.zip"), &elsewhere.0);
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": elsewhere.path("case.solidcraft")}));
    let links = run(&mut opened, "component.links", json!({}));
    assert!(links["links"].as_array().unwrap().iter().all(|l| l["found"] == true), "{links}");
}

/// A design with no file or unsaved changes, a path that isn't a .zip, and a link to a file that
/// can't be read, a device, a network location not allowed, the design itself or anything that
/// isn't a design are refused, and nothing is written. A refused link is named by its path, never
/// by what the file holds, and one judged from its text asks nothing of the disk.
#[test]
fn what_cant_be_packed_is_refused_and_nothing_is_written() {
    let folder = Folder::new("pack-refused");
    let zip = folder.path("out.zip");
    let mut s = Session::default();
    assert!(refused(&mut s, json!({"path": zip})).contains("no file yet"));
    let plate = folder.path("plate.solidcraft");
    save_box(&plate, 30.0);
    let case = folder.path("case.solidcraft");
    let mut s = assembly(&case, &[&plate]);
    for (path, why) in [
        (json!(folder.path("out.svg")), "path must be a .zip file"),
        (json!(folder.path("zip")), "path must be a .zip file"),
        (json!(format!("{}.zip", "a".repeat(MAX_PATH_BYTES))), "path must be a .zip file"),
        (json!(5), "path"),
    ] {
        let e = refused(&mut s, json!({"path": path}));
        assert!(e.contains(why), "{path}: {e}");
    }
    assert!(refused(&mut s, json!({})).contains("path"));
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1}));
    assert!(refused(&mut s, json!({"path": zip})).contains("unsaved changes"));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "file.save", json!({}));
    // Not a design: refused by its link alone.
    std::fs::write(&plate, "a key that must never be copied").unwrap();
    let e = refused(&mut s, json!({"path": zip}));
    assert!(e.contains("not a design file") && !e.contains("never be copied"), "{e}");
    std::fs::remove_file(&plate).unwrap();
    assert!(refused(&mut s, json!({"path": zip})).contains("plate.solidcraft"));
    // Links judged from their text before the disk is asked.
    for (link, why) in [(r"\\server\share\plate.solidcraft", "network location"), ("NUL", "device"), (case.as_str(), "its own link")] {
        let component = *s.doc.links.keys().next().unwrap();
        s.doc_mut().links.get_mut(&component).unwrap().path = link.to_string();
        run(&mut s, "file.save", json!({}));
        let calls = solidcraft_io::vfs::file_calls();
        let e = refused(&mut s, json!({"path": zip}));
        assert!(e.contains(why), "{link}: {e}");
        assert_eq!(solidcraft_io::vfs::file_calls(), calls, "{link}: nothing asked of the disk");
    }
    // More than a pack holds in all: refused from the files' sizes, before any is read.
    let big = folder.path("big.solidcraft");
    std::fs::File::create(&big).unwrap().set_len(solidcraft_io::MAX_PACK_BYTES + 1).unwrap();
    let component = *s.doc.links.keys().next().unwrap();
    s.doc_mut().links.get_mut(&component).unwrap().path = big.clone();
    run(&mut s, "file.save", json!({}));
    let e = refused(&mut s, json!({"path": zip}));
    assert!(e.contains("a pack holds at most 1024 MiB"), "{e}");
    std::fs::remove_file(&big).unwrap();
    assert!(!Path::new(&zip).exists());
}

/// A design saved as JSON goes in the pack as JSON, its link pointing into the pack.
#[test]
fn a_json_design_is_packed_as_json() {
    let folder = Folder::new("pack-json");
    let plate = folder.path("plate.solidcraft");
    save_box(&plate, 30.0);
    let case = folder.path("case.json");
    let mut s = assembly(&case, &[&plate]);
    let zip = folder.path("out.zip");
    assert_eq!(run(&mut s, CMD, json!({"path": zip}))["design"], json!("case.json"));
    let elsewhere = Folder::new("pack-json-unpacked");
    let entries = unpack(&zip, &elsewhere.0);
    let top = entries.iter().find(|(name, _)| name == "case.json").map(|(_, bytes)| bytes).unwrap();
    let top: Value = serde_json::from_slice(top).unwrap();
    assert!(top.to_string().contains("designs/plate.solidcraft"), "{top}");
}

/// A pack carries at most `MAX_PACK_DESIGNS` linked files: one more is refused from the links alone.
#[test]
fn a_pack_carries_at_most_its_designs() {
    let folder = Folder::new("pack-cap");
    let plate = folder.path("plate.solidcraft");
    save_box(&plate, 30.0);
    let case = folder.path("case.solidcraft");
    let mut s = assembly(&case, &[&plate]);
    let link = s.doc.links.values().next().cloned().unwrap();
    let mut files = |n: usize| {
        s.doc_mut().links = (1..=n as u64)
            .map(|i| {
                let mut link = link.clone();
                link.path = folder.path(&format!("part-{i}.solidcraft"));
                (i, link)
            })
            .collect();
        linked_files(&s, &case).map(|files| files.len()).map_err(|e| e.to_string())
    };
    assert_eq!(files(MAX_PACK_DESIGNS), Ok(MAX_PACK_DESIGNS));
    let e = files(MAX_PACK_DESIGNS + 1).unwrap_err();
    assert!(e.contains("a pack holds at most 256"), "{e}");
}
