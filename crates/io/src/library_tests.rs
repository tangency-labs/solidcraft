use super::*;
use serde_json::{Value, json};
use std::path::Path;

const ID: &str = "user.shop";

fn header(id: &str) -> Value {
    json!({"format": FORMAT, "id": id, "name": "Shop", "provenance": {"license": "CC0-1.0"}})
}
fn material(id: &str, local: &str, name: &str, density: f64) -> Entry {
    let definition = Definition::Material(LibraryMaterial { name: name.into(), density_g_cm3: density, appearance: None, physical_card: None });
    Entry::with_id(id, local, definition, Provenance::default()).unwrap()
}
fn look(id: &str, local: &str, name: &str) -> Entry {
    let definition = Definition::Appearance(LibraryLook { name: name.into(), color: [10, 20, 30], opacity: 1.0, pbr: None });
    Entry::with_id(id, local, definition, Provenance::default()).unwrap()
}
fn put(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}
fn put_json(path: &Path, value: &impl serde::Serialize) {
    put(path, serde_json::to_vec_pretty(value).unwrap());
}
/// A library folder with a manifest, two materials and one appearance.
fn shop(dir: &Path, id: &str) {
    put_json(&dir.join("Shop.sclibrary.json"), &header(id));
    put_json(&dir.join("materials/Steel S275.json"), &material(id, "s275", "Steel S275", 7.85));
    put_json(&dir.join("materials/PLA.json"), &material(id, "pla", "PLA", 1.24));
    put_json(&dir.join("appearances/Blue.json"), &look(id, "blue", "Blue"));
}
fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
/// The one library a library place holds, its place read without trouble.
fn library_at(dir: &Path) -> Library {
    library_with(dir, &LIMITS)
}
fn library_with(dir: &Path, limits: &Limits) -> Library {
    let scanned = scan_with(&Place::Library(path(dir)), limits);
    assert_eq!((scanned.status(), scanned.libraries().len()), (&Status::Available, 1), "{scanned:?}");
    scanned.libraries()[0].clone()
}
fn unavailable(status: &Status) -> Reason {
    match status {
        Status::Unavailable(reason) => reason.clone(),
        Status::Available => panic!("expected unavailable"),
    }
}
fn refused<'a>(library: &'a Library, label: &str) -> &'a Reason {
    let listed = library.entries().iter().find(|l| l.label() == label).unwrap_or_else(|| panic!("{label} not listed"));
    listed.entry().expect_err(label)
}

#[test]
fn a_library_folder_reads_back_exactly() {
    let dir = tempfile::tempdir().unwrap();
    shop(dir.path(), ID);
    let library = library_at(dir.path());
    assert_eq!(library.status(), &Status::Available);
    assert_eq!((library.id(), library.name()), (Some(ID), Some("Shop")));
    assert_eq!(library.read_only(), None);
    let mut entries: Vec<(&str, Kind, Entry)> = library.entries().iter().map(|l| (l.label(), l.kind(), l.entry().unwrap().clone())).collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(
        entries,
        vec![
            ("Blue.json", Kind::Appearance, look(ID, "blue", "Blue")),
            ("PLA.json", Kind::Material, material(ID, "pla", "PLA", 1.24)),
            ("Steel S275.json", Kind::Material, material(ID, "s275", "Steel S275", 7.85)),
        ]
    );
    // A file name is a label: the id inside is the identity.
    std::fs::rename(dir.path().join("materials/PLA.json"), dir.path().join("materials/renamed.json")).unwrap();
    let set = Set::new(vec![scan(&Place::Library(path(dir.path())))]);
    assert_eq!(set.get("user.shop/pla"), Some(&material(ID, "pla", "PLA", 1.24)));
}

#[test]
fn an_edited_entry_keeps_its_id_and_changes_its_revision() {
    let before = material(ID, "pla", "PLA", 1.24);
    let after = material(ID, "pla", "PLA", 1.25);
    assert_eq!(before.id, after.id);
    assert_ne!(before.revision, after.revision);
    let bad_local = Entry::with_id(ID, "Not An Id", before.definition.clone(), Provenance::default());
    assert!(bad_local.is_err());
}

#[test]
fn a_place_or_library_that_cannot_be_read_stays_listed_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let gone = path(&dir.path().join("gone"));
    for place in [Place::Library(gone.clone()), Place::Folder(gone)] {
        let missing = scan(&place);
        assert_eq!(unavailable(missing.status()), Reason::Missing);
        assert_eq!(missing.place(), &place);
        assert!(missing.libraries().is_empty());
    }
    // The folder itself is there; the library in it can't be used.
    let reason = |name: &str| unavailable(library_at(&dir.path().join(name)).status());

    std::fs::create_dir_all(dir.path().join("empty")).unwrap();
    assert_eq!(reason("empty"), Reason::NoManifest);

    put_json(&dir.path().join("two/A.sclibrary.json"), &header("user.a"));
    put_json(&dir.path().join("two/B.sclibrary.json"), &header("user.b"));
    assert_eq!(reason("two"), Reason::TwoManifests);

    put(&dir.path().join("damaged/D.sclibrary.json"), b"{not json");
    assert!(matches!(reason("damaged"), Reason::Invalid(_)));
    put_json(&dir.path().join("bad-id/D.sclibrary.json"), &header("Has Spaces"));
    assert!(matches!(reason("bad-id"), Reason::Invalid(_)));

    let mut h = header(ID);
    h["format"] = json!("solidcraft-library-folder/2");
    put_json(&dir.path().join("newer-version/N.sclibrary.json"), &h);
    assert_eq!(reason("newer-version"), Reason::NewerFormat);

    let mut h = header(ID);
    h["categories"] = json!(["metals"]);
    put_json(&dir.path().join("newer-field/N.sclibrary.json"), &h);
    assert_eq!(reason("newer-field"), Reason::NewerFormat);
}

/// A folder can't stand in for SolidCraft's own library, wherever it sits in the list.
#[test]
fn a_library_folder_cannot_take_solidcrafts_own_ids() {
    let dir = tempfile::tempdir().unwrap();
    for (folder, id) in [("builtin", BUILT_IN_ID), ("other", "solidcraft.metals")] {
        shop(&dir.path().join(folder), id);
        let why = unavailable(library_at(&dir.path().join(folder)).status());
        assert!(matches!(&why, Reason::Invalid(why) if why.contains("solidcraft.")), "{id}: {why:?}");
    }
    let set = Set::new(vec![scan(&Place::Library(path(&dir.path().join("builtin")))), scan(&Place::BuiltIn)]);
    assert_eq!(set.get("solidcraft.builtin/s275"), None);
    assert!(set.get("solidcraft.builtin/material-steel").is_some());
}

#[test]
fn a_bad_entry_stays_listed_as_refused_and_the_rest_of_the_library_reads() {
    let dir = tempfile::tempdir().unwrap();
    shop(dir.path(), ID);
    let m = dir.path().join("materials");
    put(&m.join("broken.json"), b"{\"id\": ");
    let mut tampered = serde_json::to_value(material(ID, "tampered", "Tampered", 2.0)).unwrap();
    tampered["definition"]["value"]["density_g_cm3"] = json!(3.0);
    put_json(&m.join("tampered.json"), &tampered);
    put_json(&m.join("foreign.json"), &material("user.other", "foreign", "Foreign", 2.0));
    put_json(&m.join("look in materials.json"), &look(ID, "misplaced", "Misplaced"));
    let mut newer = serde_json::to_value(material(ID, "newer", "Newer", 2.0)).unwrap();
    newer["definition"]["value"]["hardness"] = json!({"scale": "HB", "value": 120});
    put_json(&m.join("newer.json"), &newer);
    put(&m.join("notes.txt"), b"not an entry");

    let library = library_at(dir.path());
    assert_eq!(library.status(), &Status::Available);
    assert!(matches!(refused(&library, "broken.json"), Reason::Invalid(_)));
    assert!(matches!(refused(&library, "tampered.json"), Reason::Invalid(_)));
    assert_eq!(refused(&library, "foreign.json"), &Reason::ForeignId);
    assert_eq!(refused(&library, "look in materials.json"), &Reason::WrongKind);
    assert_eq!(refused(&library, "newer.json"), &Reason::NewerFormat);
    assert!(library.entries().iter().all(|l| l.label() != "notes.txt"));
    let set = Set::new(vec![scan(&Place::Library(path(dir.path())))]);
    assert!(set.get("user.shop/s275").is_some());
    assert!(set.get("user.shop/tampered").is_none(), "a refused entry is never used");
    assert_eq!(set.entries().filter(|(_, l)| l.entry().is_ok()).count(), 3);
}

/// A field a newer SolidCraft added deep inside an entry (here in a material card, whose reader
/// puts its own words before serde's) still reads as newer, not as damage.
#[test]
fn an_unknown_field_inside_a_material_card_reads_as_newer() {
    let dir = tempfile::tempdir().unwrap();
    shop(dir.path(), ID);
    let card = solidcraft_doc::physical_material::ValidatedCard::from_value(
        &json!({"schema_version":1,"designation":"Original synthetic card","family":"other",
        "sources":[{"id":"fixture","title":"Original invented constants","kind":"illustrative"}],
        "properties":[{"source_id":"fixture","data":{"kind":"elasticity","model":{
            "symmetry":"isotropic","young_modulus_pa":12.25e9,"poisson_ratio":0.25}}}]}),
    )
    .unwrap();
    let definition = Definition::Material(LibraryMaterial { name: "Carded".into(), density_g_cm3: 2.0, appearance: None, physical_card: Some(card) });
    let mut entry = serde_json::to_value(Entry::with_id(ID, "carded", definition, Provenance::default()).unwrap()).unwrap();
    let file = dir.path().join("materials/carded.json");
    put_json(&file, &entry);
    assert!(library_at(dir.path()).entries().iter().all(|l| l.entry().is_ok()), "the card reads as it is");
    entry["definition"]["value"]["physical_card"]["sources"][0]["edition"] = json!("2027");
    put_json(&file, &entry);
    assert_eq!(refused(&library_at(dir.path()), "carded.json"), &Reason::NewerFormat);
}

#[test]
fn limits_are_kept_at_their_boundaries() {
    let small = Limits {
        manifest_bytes: 400,
        entry_bytes: 600,
        entries_per_kind: 3,
        library_bytes: 2_000,
        files_beside_manifest: 2,
        libraries_per_folder: 2,
    };
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    put_json(&lib.join("L.sclibrary.json"), &header(ID));
    for i in 0..3 {
        put_json(&lib.join(format!("materials/m{i}.json")), &material(ID, &format!("m{i}"), &format!("M{i}"), 1.0));
    }
    let entry_size = std::fs::metadata(lib.join("materials/m0.json")).unwrap().len();
    assert!(entry_size <= small.entry_bytes, "{entry_size}");
    let read = |limits: &Limits| library_with(&lib, limits);
    assert_eq!(read(&small).status(), &Status::Available, "3 entries is the cap, not over it");

    put_json(&lib.join("materials/m3.json"), &material(ID, "m3", "M3", 1.0));
    assert_eq!(unavailable(read(&small).status()), Reason::OverLimit { cap: Cap::EntriesPerKind(Kind::Material), limit: 3 });
    std::fs::remove_file(lib.join("materials/m3.json")).unwrap();

    // One entry file at the cap reads; one byte more is refused, alone.
    let exact = Limits { entry_bytes: entry_size, ..small };
    assert!(read(&exact).entries().iter().all(|l| l.entry().is_ok()));
    let under = Limits { entry_bytes: entry_size - 1, ..small };
    assert!(read(&under).entries().iter().all(|l| matches!(l.entry(), Err(Reason::OverLimit { cap: Cap::EntryBytes, .. }))));
    assert_eq!(read(&under).status(), &Status::Available);

    // The library as a whole: the manifest and every entry file are counted.
    let manifest = std::fs::metadata(lib.join("L.sclibrary.json")).unwrap().len();
    let all = manifest + 3 * entry_size;
    assert_eq!(read(&Limits { library_bytes: all, ..small }).status(), &Status::Available);
    assert_eq!(unavailable(read(&Limits { library_bytes: all - 1, ..small }).status()), Reason::OverLimit { cap: Cap::LibraryBytes, limit: all - 1 });

    let big_manifest = Limits { manifest_bytes: manifest - 1, ..small };
    assert_eq!(unavailable(read(&big_manifest).status()), Reason::OverLimit { cap: Cap::ManifestBytes, limit: manifest - 1 });
    // The size is asked before the file is read: in the browser a read copies the stored bytes first.
    let manifest_path = path(&lib.join("L.sclibrary.json"));
    let calls = crate::vfs::file_calls();
    assert_eq!(
        read_capped(&manifest_path, Cap::ManifestBytes, manifest - 1),
        Err(Reason::OverLimit { cap: Cap::ManifestBytes, limit: manifest - 1 })
    );
    assert_eq!(crate::vfs::file_calls() - calls, 1, "one size check, nothing opened");
    assert_eq!(read_capped(&manifest_path, Cap::ManifestBytes, manifest).map(|b| b.len() as u64), Ok(manifest));
    assert_eq!(read_capped(&path(&lib.join("gone.json")), Cap::EntryBytes, 10), Err(Reason::Missing));

    // Too many files beside the manifest: the place itself can't be listed.
    put(&lib.join("README.md"), b"a note");
    put(&lib.join("LICENSE"), b"CC0");
    assert_eq!(unavailable(scan_with(&Place::Library(path(&lib)), &small).status()), Reason::OverLimit { cap: Cap::FilesBesideManifest, limit: 2 });
    assert_eq!(LIMITS.entries_per_kind, crate::catalog::MAX_ENTRIES);
    assert_eq!(LIMITS.library_bytes, crate::catalog::MAX_BYTES as u64);
}

#[test]
fn a_folder_place_lists_each_library_folder_in_name_order() {
    let dir = tempfile::tempdir().unwrap();
    let place = Place::Folder(path(dir.path()));
    let empty = scan(&place);
    assert_eq!((empty.status(), empty.libraries().len()), (&Status::Available, 0), "an empty folder place is listed, with no library");

    shop(&dir.path().join("b shop"), "user.b");
    shop(&dir.path().join("a shop"), "user.a");
    std::fs::create_dir_all(dir.path().join("plain folder/materials")).unwrap();
    put(&dir.path().join("loose.json"), b"{}");
    put_json(&dir.path().join("broken/One.sclibrary.json"), &header("user.c"));
    put_json(&dir.path().join("broken/Two.sclibrary.json"), &header("user.d"));
    let found = scan(&place);
    let ids: Vec<Option<&str>> = found.libraries().iter().map(Library::id).collect();
    assert_eq!(ids, vec![Some("user.a"), Some("user.b"), None], "a plain folder isn't a library; a broken one is listed");
    assert_eq!(unavailable(found.libraries()[2].status()), Reason::TwoManifests);

    // The cap counts subfolders, libraries or not: 4 here.
    let few = Limits { libraries_per_folder: 4, ..LIMITS };
    assert_eq!(scan_with(&place, &few).libraries().len(), 3);
    shop(&dir.path().join("c shop"), "user.e");
    let over = scan_with(&place, &few);
    assert_eq!(unavailable(over.status()), Reason::OverLimit { cap: Cap::LibrariesPerFolder, limit: 4 });
    assert!(over.libraries().is_empty());
}

#[test]
fn the_set_keeps_both_names_and_flags_a_second_library_with_the_same_id() {
    let dir = tempfile::tempdir().unwrap();
    shop(&dir.path().join("one"), "user.one");
    shop(&dir.path().join("two"), "user.two");
    shop(&dir.path().join("copy"), "user.one");
    put_json(&dir.path().join("copy/materials/extra.json"), &material("user.one", "extra", "Extra", 3.0));
    let places = ["one", "two", "copy"].map(|d| Place::Library(path(&dir.path().join(d))));
    let scans: Vec<Scanned> = places.iter().map(scan).collect();
    let set = Set::new(scans.clone());

    let pla: Vec<&str> = set.entries().filter(|(_, l)| l.label() == "PLA.json").filter_map(|(lib, _)| lib.id()).collect();
    assert_eq!(pla, vec!["user.one", "user.two"], "the same name in two libraries: both, each with its library");
    let statuses: Vec<&Status> = set.libraries().map(|(_, s)| s).collect();
    assert_eq!(statuses[2], &Status::Unavailable(Reason::DuplicateId { other: path(&dir.path().join("one")) }));
    assert!(set.get("user.one/extra").is_none(), "the later library is never used in the first one's place");

    // The verdict is the set's: the scan's own status is untouched, so the same scans merge again.
    assert_eq!(set.places()[2].libraries()[0].status(), &Status::Available);
    let reversed = Set::new(scans.into_iter().rev().collect());
    assert!(reversed.get("user.one/extra").is_some(), "the first in the list wins, whichever that is");

    // Two files with one entry id in one library: neither is used, and each names the other.
    put_json(&dir.path().join("two/materials/PLA again.json"), &material("user.two", "pla", "PLA", 1.24));
    let two = library_at(&dir.path().join("two"));
    assert_eq!(refused(&two, "PLA.json"), &Reason::DuplicateId { other: "materials/PLA again.json".into() });
    assert_eq!(refused(&two, "PLA again.json"), &Reason::DuplicateId { other: "materials/PLA.json".into() });
    assert!(Set::new(vec![scan(&places[1])]).get("user.two/pla").is_none());
    // The same file name in both kind folders, with one id: each still names the other, by its folder.
    put_json(&dir.path().join("two/materials/Common.json"), &material("user.two", "common", "Common", 1.0));
    put_json(&dir.path().join("two/appearances/Common.json"), &look("user.two", "common", "Common"));
    let two = library_at(&dir.path().join("two"));
    let common: Vec<(Kind, &Reason)> =
        two.entries().iter().filter(|l| l.label() == "Common.json").map(|l| (l.kind(), l.entry().expect_err("common"))).collect();
    assert_eq!(
        common,
        vec![
            (Kind::Material, &Reason::DuplicateId { other: "appearances/Common.json".into() }),
            (Kind::Appearance, &Reason::DuplicateId { other: "materials/Common.json".into() }),
        ]
    );
}

fn row(place: Place) -> PlaceRow {
    PlaceRow { place, read_only: false }
}

#[test]
fn a_sessions_libraries_follow_the_places_order_and_share_the_scans_they_keep() {
    let dir = tempfile::tempdir().unwrap();
    shop(&dir.path().join("mine/Mine"), "user.one");
    shop(&dir.path().join("one"), "user.one");
    shop(&dir.path().join("two"), "user.two");
    let mine = row(Place::Folder(path(&dir.path().join("mine"))));
    let [one, two] = ["one", "two"].map(|d| row(Place::Library(path(&dir.path().join(d)))));
    let built_in = Arc::new(scan(&Place::BuiltIn));
    let scans: Vec<Arc<Scanned>> = [&mine, &one, &two].iter().map(|r| Arc::new(scan(&r.place))).collect();
    let libraries = Libraries::new(Arc::clone(&built_in), Some(mine.clone()), vec![one.clone(), two.clone()], &scans, &[]);

    // The built-in library first, then My Library, then the places in their order.
    let ids: Vec<Option<&str>> = libraries.set().libraries().map(|(l, _)| l.id()).collect();
    assert_eq!(ids, vec![Some(BUILT_IN_ID), Some("user.one"), Some("user.one"), Some("user.two")]);
    let statuses: Vec<&Status> = libraries.set().libraries().map(|(_, s)| s).collect();
    let mine_library = path(&dir.path().join("mine").join("Mine"));
    assert_eq!(statuses[2], &Status::Unavailable(Reason::DuplicateId { other: mine_library }), "the earlier place wins a shared id");
    assert_eq!(libraries.rows(), [one.clone(), two.clone()]);
    assert!(libraries.places().all(|(_, state)| *state == RowState::Scanned));

    // A rescan of one place: the others' scans are the same values, shared, not copied.
    let rescanned = vec![Arc::clone(&scans[0]), Arc::clone(&scans[1]), Arc::new(scan(&two.place))];
    let again = Libraries::new(Arc::clone(&built_in), Some(mine), vec![one.clone(), two.clone()], &rescanned, &[]);
    let kept = |l: &Libraries, i: usize| Arc::ptr_eq(&l.set().places()[i], &libraries.set().places()[i]);
    assert!(kept(&again, 0) && kept(&again, 1) && kept(&again, 2) && !kept(&again, 3));

    // A place with no scan yet is listed as scanning; one whose scan couldn't start says why. Neither is made up.
    let waiting = Libraries::new(built_in, None, vec![one.clone(), two.clone()], &[], &[(two.place.clone(), "no thread".into())]);
    let states: Vec<&RowState> = waiting.places().map(|(_, s)| s).collect();
    assert_eq!(states, vec![&RowState::Scanning, &RowState::NotScanned("no thread".into())]);
    assert_eq!(waiting.set().places().len(), 1, "only the built-in scan is in the set");
}

#[test]
fn the_places_list_changes_by_one_rule() {
    let built_in = Arc::new(scan(&Place::BuiltIn));
    let [a, b, c] = ["a", "b", "c"].map(|d| row(Place::Library(format!("/lib/{d}"))));
    let mine = row(Place::Folder("/data/libraries".into()));
    let libraries = |rows: Vec<PlaceRow>| Libraries::new(Arc::clone(&built_in), Some(mine.clone()), rows, &[], &[]);
    let list = libraries(vec![a.clone(), b.clone()]);
    let add = |row: &PlaceRow, at| PlaceEdit::Add { row: row.clone(), at };

    assert_eq!(list.edit(&add(&c, None)), Ok(vec![a.clone(), b.clone(), c.clone()]));
    assert_eq!(list.edit(&add(&c, Some(0))), Ok(vec![c.clone(), a.clone(), b.clone()]));
    assert_eq!(list.edit(&PlaceEdit::Remove { path: "/lib/a".into() }), Ok(vec![b.clone()]));
    let set = |path: &str, read_only, at| PlaceEdit::Set { path: path.into(), read_only, at };
    assert_eq!(list.edit(&set("/lib/a", None, Some(1))), Ok(vec![b.clone(), a.clone()]));
    assert_eq!(list.edit(&set("/lib/b", Some(true), None)), Ok(vec![a.clone(), PlaceRow { read_only: true, ..b.clone() }]));

    // Refused, each with its reason: the list is unchanged.
    assert_eq!(list.edit(&add(&a, None)), Err("/lib/a is listed already".into()));
    assert_eq!(list.edit(&add(&row(Place::Folder("/lib/a/".into())), None)), Err("/lib/a/ is listed already".into()));
    assert_eq!(list.edit(&add(&row(Place::Library("/data/libraries".into())), None)), Err("/data/libraries is listed already".into()));
    assert_eq!(list.edit(&add(&row(Place::BuiltIn), None)), Err("the built-in library is always listed".into()));
    assert_eq!(list.edit(&add(&c, Some(3))), Err("`at` is 3, past the end of the 2 places".into()));
    assert_eq!(list.edit(&set("/lib/a", None, Some(2))), Err("`at` is 2, past the end of the 1 places".into()));
    assert_eq!(list.edit(&PlaceEdit::Remove { path: "/lib/z".into() }), Err("no place is listed at /lib/z".into()));

    // At most MAX_PLACES: the last one fits, one more doesn't.
    let many: Vec<PlaceRow> = (0..MAX_PLACES - 1).map(|i| row(Place::Library(format!("/many/{i}")))).collect();
    let full = libraries(many).edit(&add(&c, None)).unwrap();
    assert_eq!(full.len(), MAX_PLACES);
    assert_eq!(libraries(full).edit(&add(&row(Place::Library("/one/more".into())), None)), Err(format!("a list holds at most {MAX_PLACES} places")));
}

/// One folder is one place however it is spelled: on Windows, case and separators don't count.
#[cfg(windows)]
#[test]
fn a_folder_spelled_two_ways_is_one_place_on_windows() {
    let list = Libraries::new(Arc::new(scan(&Place::BuiltIn)), None, vec![row(Place::Library(r"C:\Lib".into()))], &[], &[]);
    for spelling in ["c:/lib/", r"C:\LIB\"] {
        assert!(list.edit(&PlaceEdit::Add { row: row(Place::Library(spelling.into())), at: None }).is_err(), "{spelling}");
    }
}

#[cfg(not(windows))]
#[test]
fn a_folders_case_counts_elsewhere() {
    let list = Libraries::new(Arc::new(scan(&Place::BuiltIn)), None, vec![row(Place::Library("/lib".into()))], &[], &[]);
    assert!(list.edit(&PlaceEdit::Add { row: row(Place::Library("/Lib".into())), at: None }).is_ok());
}

/// Designs and library copies will point at these ids, so they never change once shipped: a built-in
/// renamed later keeps its old id by hand. The 14 old material names keep today's values exactly
/// (card 6: they stay generic entries).
#[test]
fn the_built_in_library_has_fixed_ids_and_todays_values() {
    let scanned = scan(&Place::BuiltIn);
    assert_eq!((scanned.status(), scanned.libraries().len()), (&Status::Available, 1));
    let library = &scanned.libraries()[0];
    assert_eq!(library.status(), &Status::Available);
    assert_eq!(library.read_only(), Some(&Reason::BuiltIn));
    assert_eq!(library.id(), Some(BUILT_IN_ID));
    let usable: Vec<&Entry> = library.entries().iter().filter_map(|l| l.entry().ok()).collect();
    let ids: Vec<&str> = usable.iter().map(|e| e.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "no id twice");
    // Each entry names its table, with the licence ATTRIBUTION.md gives that table.
    assert!(usable.iter().all(|e| e.provenance.license == "MIT OR Apache-2.0"));
    let from = |source: &str| usable.iter().filter(|e| e.provenance.source_url.as_deref() == Some(source)).count();
    assert_eq!(
        [
            from("assets/materials/library.rs"),
            from("assets/appearances/library.rs"),
            from("assets/appearances/opaque_finishes.rs"),
            from("assets/appearances/starter_pack.rs")
        ],
        [14, solidcraft_doc::appearance::LIBRARY.len(), solidcraft_doc::appearance::OPAQUE_FINISHES.len(), 34]
    );
    // Written out, not read from the starter pack: renaming a preset there must fail here.
    assert_eq!(
        ids.get(40..).unwrap(),
        [
            "solidcraft.builtin/starter.wood.oak-honey",
            "solidcraft.builtin/starter.wood.oak-smoked",
            "solidcraft.builtin/starter.wood.walnut-dark",
            "solidcraft.builtin/starter.wood.maple-light",
            "solidcraft.builtin/starter.wood.cherry-warm",
            "solidcraft.builtin/starter.wood.ash-pale",
            "solidcraft.builtin/starter.wood.birch-cream",
            "solidcraft.builtin/starter.wood.teak-golden",
            "solidcraft.builtin/starter.metal.carbon-steel-polished",
            "solidcraft.builtin/starter.metal.stainless-soft-satin",
            "solidcraft.builtin/starter.metal.aluminum-bead-blast",
            "solidcraft.builtin/starter.metal.aluminum-silver-polish",
            "solidcraft.builtin/starter.metal.copper-aged-warm",
            "solidcraft.builtin/starter.metal.brass-champagne",
            "solidcraft.builtin/starter.metal.bronze-dark-cast",
            "solidcraft.builtin/starter.metal.titanium-matte",
            "solidcraft.builtin/starter.metal.gold-warm",
            "solidcraft.builtin/starter.metal.silver-cool",
            "solidcraft.builtin/starter.plastic.abs-charcoal",
            "solidcraft.builtin/starter.plastic.abs-warm-white",
            "solidcraft.builtin/starter.plastic.pla-coral",
            "solidcraft.builtin/starter.plastic.petg-cyan-opaque",
            "solidcraft.builtin/starter.plastic.nylon-ivory",
            "solidcraft.builtin/starter.plastic.pc-graphite-opaque",
            "solidcraft.builtin/starter.plastic.hdpe-natural-opaque",
            "solidcraft.builtin/starter.plastic.pom-black",
            "solidcraft.builtin/starter.plastic.tpu-matte-black",
            "solidcraft.builtin/starter.plastic.peek-amber-opaque",
            "solidcraft.builtin/starter.solar.mono-blue",
            "solidcraft.builtin/starter.solar.thin-film-dark",
            "solidcraft.builtin/starter.regolith.mare-charcoal",
            "solidcraft.builtin/starter.regolith.highland-pale",
            "solidcraft.builtin/starter.regolith.dust-neutral",
            "solidcraft.builtin/starter.regolith.agglutinate-dark",
        ],
        "the starter presets keep their own ids"
    );
    assert_eq!(
        ids.get(..40).unwrap(),
        [
            "solidcraft.builtin/material-default",
            "solidcraft.builtin/material-steel",
            "solidcraft.builtin/material-stainless-steel",
            "solidcraft.builtin/material-aluminum",
            "solidcraft.builtin/material-brass",
            "solidcraft.builtin/material-copper",
            "solidcraft.builtin/material-titanium",
            "solidcraft.builtin/material-cast-iron",
            "solidcraft.builtin/material-abs-plastic",
            "solidcraft.builtin/material-pla",
            "solidcraft.builtin/material-nylon",
            "solidcraft.builtin/material-polycarbonate",
            "solidcraft.builtin/material-oak",
            "solidcraft.builtin/material-glass",
            "solidcraft.builtin/appearance-aluminum-satin",
            "solidcraft.builtin/appearance-aluminum-anodized-black",
            "solidcraft.builtin/appearance-aluminum-anodized-blue",
            "solidcraft.builtin/appearance-steel-satin",
            "solidcraft.builtin/appearance-stainless-steel-brushed",
            "solidcraft.builtin/appearance-cast-iron",
            "solidcraft.builtin/appearance-brass-polished",
            "solidcraft.builtin/appearance-copper",
            "solidcraft.builtin/appearance-titanium",
            "solidcraft.builtin/appearance-abs-white",
            "solidcraft.builtin/appearance-abs-black",
            "solidcraft.builtin/appearance-abs-red",
            "solidcraft.builtin/appearance-abs-blue",
            "solidcraft.builtin/appearance-abs-yellow",
            "solidcraft.builtin/appearance-abs-green",
            "solidcraft.builtin/appearance-rubber-black",
            "solidcraft.builtin/appearance-glass-clear",
            "solidcraft.builtin/appearance-glass-smoked",
            "solidcraft.builtin/appearance-wood-oak",
            "solidcraft.builtin/appearance-paint-gloss-white",
            "solidcraft.builtin/appearance-steel-satin-pbr",
            "solidcraft.builtin/appearance-aluminum-satin-pbr",
            "solidcraft.builtin/appearance-copper-clean-pbr",
            "solidcraft.builtin/appearance-abs-molded-pbr",
            "solidcraft.builtin/appearance-pla-smooth-pbr",
            "solidcraft.builtin/appearance-wood-oak-uniform-pbr-preview",
        ]
    );
    // The glass recipes need real refraction: listed, refused with the reason, never a see-through stand-in.
    let glass: Vec<&Listed> = library.entries().iter().filter(|l| l.entry().is_err()).collect();
    assert_eq!(glass.len(), 6);
    assert!(glass.iter().all(|l| matches!(l.entry(), Err(Reason::Unsupported(why)) if why.contains("refraction"))));

    // Today's values for the 14 old names, exactly.
    let materials: Vec<(&str, f64, Option<([u8; 3], f64)>)> = usable
        .iter()
        .filter_map(|e| match &e.definition {
            Definition::Material(m) => Some((m.name.as_str(), m.density_g_cm3, m.appearance.as_ref().map(|a| (a.color, a.opacity)))),
            Definition::Appearance(_) => None,
        })
        .collect();
    assert_eq!(
        materials,
        vec![
            ("Default", 1.29, None),
            ("Steel", 7.85, Some(([158, 164, 172], 1.0))),
            ("Stainless Steel", 8.0, Some(([186, 191, 198], 1.0))),
            ("Aluminum", 2.70, Some(([204, 208, 214], 1.0))),
            ("Brass", 8.50, Some(([208, 172, 88], 1.0))),
            ("Copper", 8.96, Some(([204, 122, 80], 1.0))),
            ("Titanium", 4.43, Some(([150, 148, 158], 1.0))),
            ("Cast Iron", 7.20, Some(([104, 104, 110], 1.0))),
            ("ABS Plastic", 1.06, Some(([222, 222, 216], 1.0))),
            ("PLA", 1.24, Some(([110, 160, 222], 1.0))),
            ("Nylon", 1.14, Some(([236, 230, 214], 1.0))),
            ("Polycarbonate", 1.20, Some(([196, 216, 232], 1.0))),
            ("Oak", 0.75, Some(([190, 146, 92], 1.0))),
            ("Glass", 2.50, Some(([170, 208, 218], 0.25))),
        ]
    );
}

/// A link inside a place is never followed: a library can't reach files outside it.
#[cfg(unix)]
#[test]
fn links_inside_a_place_are_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    shop(&outside, "user.outside");
    let lib = dir.path().join("places/lib");
    shop(&lib, ID);
    std::os::unix::fs::symlink(outside.join("materials/PLA.json"), lib.join("materials/linked.json")).unwrap();
    // A link named `appearances` is not the library's appearances folder.
    std::fs::remove_dir_all(lib.join("appearances")).unwrap();
    std::os::unix::fs::symlink(outside.join("appearances"), lib.join("appearances")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.path().join("places/linked library")).unwrap();
    let found = scan(&Place::Folder(path(&dir.path().join("places"))));
    assert_eq!(found.libraries().len(), 1, "the linked library folder is not one of the place's");
    let library = &found.libraries()[0];
    assert!(library.entries().iter().all(|l| l.label() != "linked.json"));
    assert!(library.entries().iter().all(|l| l.kind() == Kind::Material), "the linked appearances folder is not read");
}

/// A library place follows the network rules a design's links follow.
#[test]
fn a_library_place_on_a_network_location_is_refused() {
    let calls = crate::vfs::file_calls();
    for place in [Place::Library(r"\\evil.example\lib".into()), Place::Folder(r"\\evil.example\libs".into())] {
        let found = scan(&place);
        let reason = match found.status() {
            Status::Unavailable(reason) => reason.to_string(),
            Status::Available => String::new(),
        };
        assert!(reason.contains(r"network location \\evil.example"), "{place:?}: {reason}");
        assert!(found.libraries().is_empty(), "{place:?}");
    }
    assert_eq!(crate::vfs::file_calls(), calls);
}

/// Seeded fuzz (the readers' mutations, #469): hostile place lists and hostile library files. Nothing
/// panics, every cap holds, and every bad input is reported with its reason: a place that can't be
/// read is listed with why, and every entry file that is there is listed, used or refused.
#[test]
fn hostile_place_lists_and_library_files_are_refused_with_a_reason() {
    // Place lists: seeded edits over awkward folder names, from lists of 0, 1, 64 and 65 places.
    let long = format!("/lib/{}", "n".repeat(5_000));
    let names = ["/lib/a", "/lib/a/", "/LIB/A", r"C:\Lib", "c:/lib/", "", " ", "/lib/a/../a", "/lib/ünï cödé", "/lib/\u{202e}txt", long.as_str()];
    let built_in = Arc::new(scan(&Place::BuiltIn));
    let mut seed = 0x0012_9820_5eed_u64;
    let mut next = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let keys = |rows: &[PlaceRow]| {
        let mut keys: Vec<String> = rows.iter().filter_map(|r| r.place.path().map(place_key)).collect();
        keys.sort_unstable();
        keys
    };
    for start in [0, 1, MAX_PLACES, MAX_PLACES + 1] {
        let mut rows: Vec<PlaceRow> = (0..start).map(|i| row(Place::Library(format!("/many/{i}")))).collect();
        for step in 0..2_000 {
            let path = format!("{}{}", names[next(names.len())], ["", "/x", "/0", "/1"][next(4)]);
            let place = if next(2) == 0 { Place::Library(path.clone()) } else { Place::Folder(path.clone()) };
            let at = (next(2) == 0).then(|| next(MAX_PLACES + 3));
            let edit = match next(3) {
                0 => PlaceEdit::Add { row: PlaceRow { place, read_only: next(2) == 0 }, at },
                1 => PlaceEdit::Remove { path: path.clone() },
                _ => PlaceEdit::Set { path: path.clone(), read_only: Some(next(2) == 0), at },
            };
            let case = format!("start {start}, step {step}: {edit:?}");
            match Libraries::new(Arc::clone(&built_in), None, rows.clone(), &[], &[]).edit(&edit) {
                Err(why) => assert!(!why.is_empty(), "{case}: refused with no reason"),
                Ok(after) => {
                    assert!(after.len() <= rows.len() || after.len() <= MAX_PLACES, "{case}: {} places", after.len());
                    let mut unique = keys(&after);
                    unique.dedup();
                    assert_eq!(unique.len(), after.len(), "{case}: a folder listed twice");
                    let listed = keys(&after).contains(&place_key(&path));
                    assert_eq!(listed, !matches!(edit, PlaceEdit::Remove { .. }), "{case}");
                    rows = after;
                }
            }
        }
    }

    // Places that can't be read: each is listed with its reason, never an empty success.
    let dir = tempfile::tempdir().unwrap();
    put(&dir.path().join("a file"), b"not a folder");
    for folder in [
        path(&dir.path().join("missing")),
        path(&dir.path().join("a file")),
        path(&dir.path().join("n".repeat(300))),
        String::new(),
        "relative/lib".into(),
    ] {
        for place in [Place::Library(folder.clone()), Place::Folder(folder.clone())] {
            let found = scan(&place);
            assert!(!unavailable(found.status()).to_string().is_empty(), "{place:?}");
            assert!(found.libraries().is_empty(), "{place:?}");
        }
    }
    // Never read from wherever SolidCraft started: an empty or relative folder is refused by its text.
    for folder in ["", "relative/lib"] {
        let why = Reason::Unreadable("a library place needs an absolute path".into());
        assert_eq!(unavailable(scan(&Place::Library(folder.into())).status()), why, "{folder:?}");
    }
    let odd = dir.path().join("ünï cödé lib");
    shop(&odd, ID);
    assert_eq!(library_at(&odd).entries().iter().filter(|l| l.entry().is_ok()).count(), 3, "an odd folder name reads");

    // A folder place at its cap: 64 subfolders are read, 65 are refused as over it, not cut short.
    let many = dir.path().join("many");
    for i in 0..LIMITS.libraries_per_folder {
        std::fs::create_dir_all(many.join(format!("{i:02}"))).unwrap();
    }
    shop(&many.join("00"), "user.zero");
    assert_eq!(scan(&Place::Folder(path(&many))).libraries().len(), 1);
    std::fs::create_dir_all(many.join("64")).unwrap();
    let limit = LIMITS.libraries_per_folder as u64;
    assert_eq!(unavailable(scan(&Place::Folder(path(&many))).status()), Reason::OverLimit { cap: Cap::LibrariesPerFolder, limit });

    // Lies the mutations rarely make: each entry is refused with its reason, the rest still read.
    let lib = dir.path().join("lies");
    shop(&lib, ID);
    let m = lib.join("materials");
    let mut huge = serde_json::to_value(material(ID, "huge", "Huge", 1.0)).unwrap();
    huge["definition"]["value"]["density_g_cm3"] = json!(f64::MAX);
    huge["definition"]["value"]["physical_card"] = json!(0);
    let huge = serde_json::to_string(&huge).unwrap();
    let overflow = huge.replace("1.7976931348623157e+308", "1e400");
    let deep = huge.replace("\"physical_card\":0", &format!("\"physical_card\":{}{}", "[".repeat(100_000), "]".repeat(100_000)));
    assert!(overflow != huge && deep != huge, "{huge}");
    put(&m.join("empty.json"), b"");
    put(&m.join("deep.json"), deep);
    put(&m.join("huge.json"), &huge);
    put(&m.join("overflow.json"), overflow);
    put(&m.join("big.json"), vec![b' '; LIMITS.entry_bytes as usize + 1]);
    let library = library_at(&lib);
    for label in ["empty.json", "deep.json", "huge.json", "overflow.json", "big.json"] {
        assert!(!refused(&library, label).to_string().is_empty(), "{label}");
    }
    assert_eq!(refused(&library, "big.json"), &Reason::OverLimit { cap: Cap::EntryBytes, limit: LIMITS.entry_bytes });
    assert_eq!(library.entries().iter().filter(|l| l.entry().is_ok()).count(), 3);
    // The same library id in two places: the set flags the later one.
    let twin = dir.path().join("twin");
    shop(&twin, ID);
    let set = Set::new(vec![scan(&Place::Library(path(&lib))), scan(&Place::Library(path(&twin)))]);
    assert!(matches!(set.libraries().nth(1), Some((_, Status::Unavailable(Reason::DuplicateId { .. })))));

    // Mutated manifests and entry files: each case reads or is refused with its reason, and every
    // entry file that is there is listed.
    let fuzz = dir.path().join("fuzz");
    shop(&fuzz, ID);
    let on_disk = || {
        let mut labels: Vec<String> = ["materials", "appearances"]
            .iter()
            .flat_map(|kind| std::fs::read_dir(fuzz.join(kind)).unwrap())
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        labels.sort_unstable();
        labels
    };
    for (file, seed) in [("Shop.sclibrary.json", 0x5c1), ("materials/PLA.json", 0x5c2), ("appearances/Blue.json", 0x5c3)] {
        let file = fuzz.join(file);
        let good = std::fs::read(&file).unwrap();
        solidcraft_testkit::survives(&path(&file), &solidcraft_testkit::mutations(&good, true, seed), 2 * LIMITS.entries_per_kind, |bytes| {
            put(&file, bytes);
            let found = scan(&Place::Library(path(&fuzz)));
            let Some(library) = found.libraries().first() else {
                assert!(!unavailable(found.status()).to_string().is_empty());
                return None;
            };
            if let Status::Unavailable(why) = library.status() {
                assert!(!why.to_string().is_empty());
                return None;
            }
            let mut labels: Vec<String> = library.entries().iter().map(|l| l.label().to_string()).collect();
            labels.sort_unstable();
            assert_eq!(labels, on_disk(), "every entry file is listed, used or refused");
            Some(labels.len())
        });
        put(&file, &good);
    }

    // Writes with hostile names: each is refused with a reason, or reads back as it was written.
    let writes = dir.path().join("writes");
    shop(&writes, ID);
    let mut libraries = libraries_of(vec![row(Place::Library(path(&writes)))]);
    for name in names.iter().chain(["CON", "..", "a/b", "\u{202e}gpj.exe", "~1", "Steel", "steel"].iter()) {
        let definition = Definition::Material(LibraryMaterial { name: name.to_string(), density_g_cm3: 1.0, appearance: None, physical_card: None });
        match libraries.add_entry(ID, definition.clone(), Provenance::default()) {
            Err(why) => assert!(!why.is_empty(), "{name:?}"),
            Ok((next, written)) => {
                let entry = next.set().get(written.entry.as_deref().unwrap_or_default()).unwrap_or_else(|| panic!("{name:?}: {written:?}"));
                assert_eq!(entry.definition, definition, "{name:?}");
                assert!(writes.join(&written.file).is_file(), "{name:?}: {written:?}");
                libraries = next;
            }
        }
    }
}
/// The libraries of `rows`, each place scanned, as a session holds them.
fn libraries_of(rows: Vec<PlaceRow>) -> Libraries {
    let scans: Vec<Arc<Scanned>> = rows.iter().map(|r| Arc::new(scan(&r.place))).collect();
    Libraries::new(Arc::new(scan(&Place::BuiltIn)), None, rows, &scans, &[])
}
fn library_in<'a>(libraries: &'a Libraries, id: &str) -> Option<&'a Library> {
    libraries.set().libraries().find(|(l, _)| l.id() == Some(id)).map(|(l, _)| l)
}
fn steel(density: f64) -> Definition {
    Definition::Material(LibraryMaterial { name: "Steel".into(), density_g_cm3: density, appearance: None, physical_card: None })
}

/// A new library gets a folder of its own in a folder place, or takes an empty library place, and
/// the next command finds it. A name made twice gets a second folder and id, never the first's.
#[test]
fn a_new_library_is_made_in_its_own_folder_and_read_back_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let place = path(&dir.path().join("libraries"));
    let libraries = libraries_of(vec![row(Place::Folder(place.clone()))]);
    let (libraries, made) = libraries.create_library(&place, "Shop", Provenance::default()).unwrap();
    let expected = write::Written { library: "user.shop".into(), entry: None, revision: None, file: "Shop.sclibrary.json".into() };
    assert_eq!(made, expected);
    let folder = dir.path().join("libraries").join("Shop");
    assert!(folder.join("Shop.sclibrary.json").is_file() && folder.join("materials").is_dir() && folder.join("appearances").is_dir());
    assert_eq!(library_in(&libraries, "user.shop").and_then(Library::folder), Some(path(&folder).as_str()));
    let (libraries, again) = libraries.create_library(&place, "shop", Provenance::default()).unwrap();
    assert_eq!(again.library, "user.shop-2");
    assert!(dir.path().join("libraries/shop (2)/shop.sclibrary.json").is_file());
    assert_eq!(libraries.set().libraries().filter(|(_, s)| **s == Status::Available).count(), 3, "the built-in library and both");

    // A library place: an empty folder becomes the library; a second one there, or a folder with
    // anything in it, is refused.
    let empty = dir.path().join("empty");
    let full = dir.path().join("full");
    std::fs::create_dir_all(&empty).unwrap();
    put(&full.join("notes.txt"), b"someone's folder");
    let places = [&empty, &full].map(|d| row(Place::Library(path(d))));
    let libraries = libraries_of(places.to_vec());
    let (libraries, _) = libraries.create_library(&path(&empty), "Bench", Provenance::default()).unwrap();
    assert!(empty.join("Bench.sclibrary.json").is_file());
    let err = libraries.create_library(&path(&empty), "Other", Provenance::default()).unwrap_err();
    assert!(err.contains("holds a library already"), "{err}");
    let err = libraries.create_library(&path(&full), "Other", Provenance::default()).unwrap_err();
    assert!(err.contains("isn't empty"), "{err}");
    assert_eq!(std::fs::read_dir(&full).unwrap().count(), 1, "nothing written");

    // Refused before anything is written: a read-only place, a place not listed, a bad name.
    let locked = dir.path().join("locked");
    let libraries = libraries_of(vec![PlaceRow { place: Place::Folder(path(&locked)), read_only: true }]);
    assert!(libraries.create_library(&path(&locked), "Shop", Provenance::default()).unwrap_err().contains("read-only"));
    assert!(!locked.exists());
    assert!(libraries.create_library(&path(&dir.path().join("elsewhere")), "Shop", Provenance::default()).unwrap_err().contains("no place"));
    for name in [String::new(), "n".repeat(129)] {
        assert!(libraries_of(vec![row(Place::Folder(place.clone()))]).create_library(&place, &name, Provenance::default()).is_err(), "{name:?}");
    }
}

/// A write outside the caller's folders is refused before anything is made, and the refusal names
/// the folder once (the folder check's own words, not the path again in front of them).
#[test]
fn a_write_outside_the_callers_folders_names_its_folder_once() {
    let dir = tempfile::tempdir().unwrap();
    let place = dir.path().join("libraries");
    std::fs::create_dir_all(&place).unwrap();
    let libraries = libraries_of(vec![row(Place::Folder(path(&place)))]);
    let elsewhere = crate::vfs::PathScope::new([dir.path().join("elsewhere")]);
    let err = crate::vfs::scoped(Some(elsewhere), || libraries.create_library(&path(&place), "Shop", Provenance::default())).unwrap_err();
    assert!(err.contains("outside the folders"), "{err}");
    assert_eq!(err.matches(path(&place).as_str()).count(), 1, "{err}");
    assert_eq!(std::fs::read_dir(&place).unwrap().count(), 0, "nothing written");
}

/// An entry is added under a new id, changed and removed only from the revision the caller read,
/// and copied with its provenance; a removed file goes to the library's bin, which no scan reads.
#[test]
fn entries_are_added_changed_copied_and_moved_to_the_bin_by_the_revision_read() {
    let dir = tempfile::tempdir().unwrap();
    shop(dir.path(), ID);
    let libraries = libraries_of(vec![row(Place::Library(path(dir.path())))]);
    let (libraries, added) = libraries.add_entry(ID, steel(7.85), Provenance::default()).unwrap();
    assert_eq!((added.entry.as_deref(), added.file.as_str()), (Some("user.shop/material-steel"), "materials/Steel.json"));
    let revision = added.revision.clone().unwrap();
    assert_eq!(libraries.set().get("user.shop/material-steel").map(|e| &e.revision), Some(&revision), "the next command sees it");
    let (libraries, twin) = libraries.add_entry(ID, steel(7.85), Provenance::default()).unwrap();
    assert_eq!((twin.entry.as_deref(), twin.file.as_str()), (Some("user.shop/material-steel-2"), "materials/Steel (2).json"));

    // A change names the revision it was made from: any other is refused, and the file stays.
    let stale = "0".repeat(64);
    let err = libraries.edit_entry("user.shop/material-steel", &stale, steel(7.9), None).unwrap_err();
    assert!(err.contains("changed since it was read"), "{err}");
    let (libraries, edited) = libraries.edit_entry("user.shop/material-steel", &revision, steel(7.9), None).unwrap();
    assert_eq!((edited.entry.as_deref(), edited.file.as_str()), (Some("user.shop/material-steel"), "materials/Steel.json"));
    assert_ne!(edited.revision.as_ref(), Some(&revision), "the id stays, the revision follows the content");
    assert!(libraries.edit_entry("user.shop/material-steel", &revision, steel(8.0), None).is_err(), "the old revision is stale now");
    let blue = look(ID, "x", "Blue").definition;
    let current = edited.revision.clone().unwrap();
    assert!(libraries.edit_entry("user.shop/material-steel", &current, blue, None).unwrap_err().contains("kind can't change"));

    // A copy from the built-in library keeps its definition and provenance, under an id of this library.
    let source = libraries.set().entries().filter(|(l, _)| l.id() == Some(BUILT_IN_ID)).find_map(|(_, e)| e.entry().ok()).unwrap().clone();
    let (libraries, copied) = libraries.copy_entry(&source.id, ID).unwrap();
    let copy = libraries.set().get(copied.entry.as_deref().unwrap()).unwrap();
    assert!(copy.id.starts_with("user.shop/"));
    assert_eq!((&copy.definition, &copy.provenance), (&source.definition, &source.provenance));

    // Removed: into the bin, out of every list. A second file of the same name never replaces it there.
    assert!(libraries.remove_entry("user.shop/material-steel", &revision).unwrap_err().contains("changed since it was read"));
    let (libraries, removed) = libraries.remove_entry("user.shop/material-steel", &current).unwrap();
    assert_eq!(removed.file, format!("{}/materials/Steel.json", write::BIN));
    assert!(!dir.path().join("materials/Steel.json").exists());
    assert!(libraries.set().get("user.shop/material-steel").is_none());
    let (libraries, again) = libraries.add_entry(ID, steel(7.7), Provenance::default()).unwrap();
    assert_eq!(again.file, "materials/Steel.json");
    let (libraries, _) = libraries.remove_entry(again.entry.as_deref().unwrap(), again.revision.as_deref().unwrap()).unwrap();
    let bin = dir.path().join(write::BIN).join("materials");
    assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 2);
    assert!(bin.join("Steel (2).json").is_file());
    // The bin is not a library's: a fresh scan lists only the kind folders.
    let fresh = library_at(dir.path());
    assert_eq!(fresh.entries().len(), library_in(&libraries, ID).unwrap().entries().len());
}

/// Writes go only where they may, and only over the file a scan read: never the built-in library,
/// a read-only place, an unavailable library, a file changed or removed since, nor past a cap.
#[test]
fn writes_refuse_read_only_places_the_built_in_library_and_files_changed_since_they_were_read() {
    let dir = tempfile::tempdir().unwrap();
    let (open, locked, twin) = (dir.path().join("open"), dir.path().join("locked"), dir.path().join("twin"));
    shop(&open, ID);
    shop(&locked, "user.locked");
    shop(&twin, ID);
    let rows =
        vec![row(Place::Library(path(&open))), PlaceRow { place: Place::Library(path(&locked)), read_only: true }, row(Place::Library(path(&twin)))];
    let libraries = libraries_of(rows);
    let files = |d: &Path| std::fs::read_dir(d.join("materials")).unwrap().count();

    assert!(libraries.add_entry("user.locked", steel(7.85), Provenance::default()).unwrap_err().contains("read-only place"));
    assert_eq!(files(&locked), 2);
    assert_eq!(libraries.add_entry(BUILT_IN_ID, steel(7.85), Provenance::default()).unwrap_err(), Reason::BuiltIn.to_string());
    assert!(libraries.add_entry("user.nobody", steel(7.85), Provenance::default()).unwrap_err().contains("no library"));
    // The second library with an id is unavailable, so the first is the one written.
    let (_, written) = libraries.add_entry(ID, steel(7.85), Provenance::default()).unwrap();
    assert_eq!((files(&open), files(&twin)), (3, 2), "{written:?}");

    // Changed or removed behind the session's back: refused, the file as the other writer left it.
    let pla = material(ID, "pla", "PLA", 1.24);
    put_json(&open.join("materials/PLA.json"), &material(ID, "pla", "PLA", 1.3));
    let err = libraries.edit_entry(&pla.id, &pla.revision, steel(1.0), None).unwrap_err();
    assert!(err.contains("changed since it was read"), "{err}");
    assert_eq!(std::fs::read(open.join("materials/PLA.json")).unwrap(), serde_json::to_vec_pretty(&material(ID, "pla", "PLA", 1.3)).unwrap());
    std::fs::remove_file(open.join("materials/PLA.json")).unwrap();
    assert!(libraries.remove_entry(&pla.id, &pla.revision).unwrap_err().contains("removed since it was read"));

    // A write that would take the library past its cap would leave it unavailable: refused. Files a
    // scan refuses still count, as a scan counts every byte it reads.
    let full = dir.path().join("full");
    shop(&full, "user.full");
    let room = |used: u64| (LIMITS.library_bytes - used) as usize;
    let used = || library_in(&libraries_of(vec![row(Place::Library(path(&full)))]), "user.full").unwrap().bytes;
    for i in 0..15 {
        put(&full.join(format!("materials/pad-{i:02}.json")), vec![b' '; LIMITS.entry_bytes as usize]);
    }
    put(&full.join("materials/pad-last.json"), vec![b' '; room(used()) - 100]);
    let libraries = libraries_of(vec![row(Place::Library(path(&full)))]);
    assert!(library_in(&libraries, "user.full").is_some_and(|l| l.status() == &Status::Available), "at its cap, still read");
    assert!(libraries.add_entry("user.full", steel(7.85), Provenance::default()).unwrap_err().contains("would be larger"));

    // A kind folder at its cap: one more file would make the library unavailable.
    let many = dir.path().join("many");
    shop(&many, "user.many");
    for i in 0..LIMITS.entries_per_kind - 2 {
        put(&many.join(format!("materials/{i:04}.txt")), b"");
    }
    let libraries = libraries_of(vec![row(Place::Library(path(&many)))]);
    assert!(libraries.add_entry("user.many", steel(7.85), Provenance::default()).unwrap_err().contains("holds the most entries"));
}

/// A library place on a network location takes no write either, and nothing is asked of the network.
#[test]
fn a_library_cannot_be_made_on_a_network_location() {
    let place = r"\\evil.example\libs";
    let libraries = libraries_of(vec![row(Place::Folder(place.into()))]);
    let calls = crate::vfs::file_calls();
    let err = libraries.create_library(place, "Shop", Provenance::default()).unwrap_err();
    assert!(err.contains(r"network location \\evil.example"), "{err}");
    assert_eq!(crate::vfs::file_calls(), calls);
}

/// A change never writes through a link at the entry's name, even one to a file with the same
/// content: the file the link points at stays as it was.
#[cfg(unix)]
#[test]
fn a_change_never_writes_through_a_link_at_the_entrys_name() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    shop(&lib, ID);
    let libraries = libraries_of(vec![row(Place::Library(path(&lib)))]);
    let pla = material(ID, "pla", "PLA", 1.24);
    let outside = dir.path().join("outside.json");
    std::fs::copy(lib.join("materials/PLA.json"), &outside).unwrap();
    let before = std::fs::read(&outside).unwrap();
    std::fs::remove_file(lib.join("materials/PLA.json")).unwrap();
    std::os::unix::fs::symlink(&outside, lib.join("materials/PLA.json")).unwrap();
    let _ = libraries.edit_entry(&pla.id, &pla.revision, steel(1.0), None);
    let _ = libraries.remove_entry(&pla.id, &pla.revision);
    assert_eq!(std::fs::read(&outside).unwrap(), before, "the linked file is untouched");
}

/// A new file's name works on every system SolidCraft runs on, hides no format character, and is
/// never one the folder has, whatever the case.
#[test]
fn a_new_files_name_is_safe_on_every_system_and_never_one_already_there() {
    let free = |_: &str| false;
    for (name, file) in [
        ("Steel S275", "Steel S275.json"),
        ("a/b\\c:d*e?f\"g<h>i|j", "a_b_c_d_e_f_g_h_i_j.json"),
        ("CON", "_CON.json"),
        ("com1.x", "_com1.x.json"),
        (" .hidden. ", "hidden.json"),
        ("", "entry.json"),
        ("...", "entry.json"),
        ("PROGRA~1", "PROGRA-1.json"),
        ("left\u{202e}txt.exe", "left_txt.exe.json"),
        ("zero\u{200b}width\u{feff}", "zero_width_.json"),
        ("tab\there", "tab_here.json"),
    ] {
        assert_eq!(write::file_name(name, free).as_deref(), Some(file), "{name:?}");
    }
    let long = write::file_name(&"é".repeat(300), free).unwrap();
    assert!(long.len() <= 105 && long.ends_with(".json"), "{long}");
    assert_eq!(write::file_name("Steel", |n| n == "steel.json").as_deref(), Some("Steel (2).json"));
    assert_eq!(write::file_name("Steel", |_| true), None);
}

/// A changed-only refresh scans a place again only when a listing its scan made comes out
/// different: an entry file's length or time, a file or folder added or gone, in a library place
/// and in a folder place. A place whose files are as they were keeps its scan.
#[test]
fn a_place_is_scanned_again_only_when_a_listing_changed() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("Shop");
    shop(&lib, ID);
    let first = scan(&Place::Library(path(&lib)));
    assert_eq!(rescan_if_changed(&first), None, "nothing changed");
    put_json(&lib.join("materials/PLA.json"), &material(ID, "pla", "PLA, dried", 1.25));
    let edited = rescan_if_changed(&first).expect("an edited entry file");
    assert_ne!(edited, first);
    assert_eq!(rescan_if_changed(&edited), None, "the new scan's own listings");
    // The same length, a later time: the time alone tells.
    let file = lib.join("materials/PLA.json");
    let text = std::fs::read_to_string(&file).unwrap().replace("1.25", "1.26");
    let later = std::fs::metadata(&file).unwrap().modified().unwrap() + std::time::Duration::from_secs(5);
    std::fs::write(&file, text).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(later).unwrap();
    let edited = rescan_if_changed(&edited).expect("an edit that kept the length");
    put_json(&lib.join("appearances/Red.json"), &look(ID, "red", "Red"));
    let added = rescan_if_changed(&edited).expect("a new entry file");
    assert_eq!(added.libraries()[0].entries().len(), 4);
    std::fs::remove_file(lib.join("appearances/Red.json")).unwrap();
    let removed = rescan_if_changed(&added).expect("an entry file gone");
    std::fs::create_dir(lib.join("textures")).unwrap();
    assert!(rescan_if_changed(&removed).is_some(), "a new folder beside the manifest");

    // A folder place: a new library, and a folder that becomes one when a manifest appears in it.
    let folder = dir.path().join("all");
    shop(&folder.join("a"), "user.a");
    let first = scan(&Place::Folder(path(&folder)));
    assert_eq!(rescan_if_changed(&first), None);
    std::fs::create_dir_all(folder.join("b")).unwrap();
    let empty = rescan_if_changed(&first).expect("a new subfolder");
    assert_eq!(empty.libraries().len(), 1, "a folder with no manifest isn't a library");
    put_json(&folder.join("b/B.sclibrary.json"), &header("user.b"));
    assert_eq!(rescan_if_changed(&empty).expect("a manifest in the subfolder").libraries().len(), 2);
    assert_eq!(rescan_if_changed(&scan(&Place::BuiltIn)), None, "the built-in library never changes");
}

/// A place that goes missing, turns unreadable or goes over a cap between the scan and the check
/// counts as changed, and so does one that comes back: the check lists through the scan's own path
/// checks and caps, so it reaches the same outcome the next scan would.
#[test]
fn a_place_that_turns_unreadable_or_over_a_cap_or_comes_back_counts_as_changed() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("Shop");
    shop(&lib, ID);
    let place = Place::Library(path(&lib));
    let first = scan(&place);
    let away = dir.path().join("away");
    std::fs::rename(&lib, &away).unwrap();
    let missing = rescan_if_changed(&first).expect("the folder gone");
    assert_eq!(unavailable(missing.status()), Reason::Missing);
    assert_eq!(rescan_if_changed(&missing), None, "still missing: nothing changed");
    put(&lib, b"a file where the folder was");
    let unreadable = rescan_if_changed(&missing).expect("unreadable now");
    assert!(matches!(unavailable(unreadable.status()), Reason::Unreadable(_)), "{unreadable:?}");
    assert_eq!(rescan_if_changed(&unreadable), None, "the same reason again");
    std::fs::remove_file(&lib).unwrap();
    std::fs::rename(&away, &lib).unwrap();
    assert_eq!(rescan_if_changed(&unreadable).expect("back").status(), &Status::Available);

    // Over a cap: the check uses the cap the scan used, both ways.
    // Two subfolders and the manifest: within two of each.
    let small = Limits { files_beside_manifest: 2, ..LIMITS };
    let within = scan_with(&place, &small);
    assert_eq!(within.libraries()[0].status(), &Status::Available);
    put(&lib.join("notes.txt"), b"a second file");
    assert_eq!(rescan_with(&within, &small).expect("a file more").status(), &Status::Available);
    let within = scan_with(&place, &small);
    put(&lib.join("more notes.txt"), b"a third file");
    let over = rescan_with(&within, &small).expect("over the cap");
    assert_eq!(unavailable(over.status()), Reason::OverLimit { cap: Cap::FilesBesideManifest, limit: 2 });
    assert_eq!(rescan_with(&over, &small), None, "still over");
    std::fs::remove_file(lib.join("more notes.txt")).unwrap();
    assert_eq!(rescan_with(&over, &small).expect("within again").status(), &Status::Available);
}

/// The limit of a changed-only check, kept on purpose: an edit that keeps a file's length and its
/// time (a drive that keeps time in 2 s steps, FAT, and an edit inside one step) isn't seen. The
/// Refresh button scans again in full; this test pins the limit so a change to it is a choice.
#[test]
fn a_same_length_edit_at_the_same_time_is_left_to_the_full_rescan() {
    let dir = tempfile::tempdir().unwrap();
    shop(dir.path(), ID);
    let file = dir.path().join("materials/PLA.json");
    let when = std::fs::metadata(&file).unwrap().modified().unwrap();
    let first = scan(&Place::Library(path(dir.path())));
    let text = std::fs::read_to_string(&file).unwrap().replace("1.24", "1.25");
    std::fs::write(&file, text).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(when).unwrap();
    assert_eq!(rescan_if_changed(&first), None);
    let full = scan(first.place());
    assert_ne!(full, first, "the full rescan reads the edit");
}

/// A write leaves its place's scan current: the check after it finds nothing to scan again, so a
/// window that checks on focus doesn't read a library it just wrote.
#[test]
fn a_write_leaves_its_place_current_for_the_check() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("libraries");
    shop(&folder.join("Shop"), ID);
    let libraries = libraries_of(vec![row(Place::Folder(path(&folder)))]);
    let (libraries, _) = libraries.add_entry(ID, steel(7.85), Provenance::default()).unwrap();
    let written = libraries.set().places().get(1).unwrap();
    assert_eq!(rescan_if_changed(written), None, "an added entry");
    let (libraries, _) = libraries.create_library(&path(&folder), "Bench", Provenance::default()).unwrap();
    assert_eq!(rescan_if_changed(libraries.set().places().get(1).unwrap()), None, "a new library");
}
