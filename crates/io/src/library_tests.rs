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
