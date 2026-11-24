use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use crate::scenario::Harness;

/// A one-library folder in the temporary folder, removed when dropped.
struct Shop(PathBuf);

impl Shop {
    fn new(name: &str) -> Self {
        let shop = Shop(std::env::temp_dir().join(format!("sc-library-places-{name}-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&shop.0);
        shop.named(&format!("user.{name}"), "Shop");
        shop
    }
    /// Writes its manifest: the library's id and name.
    fn named(&self, id: &str, name: &str) {
        std::fs::create_dir_all(&self.0).unwrap();
        let manifest = json!({"format": "solidcraft-library-folder/1", "id": id, "name": name, "provenance": {"license": "CC0-1.0"}});
        std::fs::write(self.0.join("Shop.sclibrary.json"), manifest.to_string()).unwrap();
    }
    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Shop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(h: &mut Harness, id: &str, params: Value) -> Value {
    let out = h.call("engine.execute", json!({ "command": id, "params": params }));
    assert!(out["error"].is_null(), "{id}: {out}");
    out["result"].clone()
}

/// The places as `library.places.list` gives them, the built-in library left out.
fn places(h: &mut Harness) -> Vec<Value> {
    let listed = run(h, "library.places.list", json!({}));
    listed["places"].as_array().map(|p| p.iter().skip(1).cloned().collect()).unwrap_or_default()
}

fn finish(h: &mut Harness) {
    crate::library_places::finish(&mut h.app, &h.ctx, Duration::from_secs(30));
}

/// A place added through its command is in the preferences at once, the next command sees it,
/// and its libraries are listed once its scan is done. A rescan reads the folder again.
#[test]
fn a_place_added_through_its_command_is_scanned_and_listed() {
    let shop = Shop::new("added");
    let mut h = Harness::new();
    run(&mut h, "library.places.add", json!({"path": shop.path(), "kind": "library"}));
    assert_eq!(
        serde_json::to_value(&h.app.preferences.library_places).unwrap(),
        json!([{"place": {"kind": "library", "path": shop.path()}, "read_only": false}])
    );
    assert_eq!(h.app.session.libraries.rows(), h.app.preferences.library_places.as_slice(), "the next command sees the place at once");

    h.frame();
    finish(&mut h);
    let listed = places(&mut h);
    assert_eq!((&listed[0]["scan"], &listed[0]["libraries"][0]["id"]), (&json!("scanned"), &json!("user.added")), "{listed:?}");
    assert_eq!(listed[0]["libraries"][0]["available"], json!(true));

    shop.named("user.added", "Renamed shop");
    run(&mut h, "library.places.refresh", json!({"path": shop.path()}));
    h.frame();
    finish(&mut h);
    assert_eq!(places(&mut h)[0]["libraries"][0]["name"], json!("Renamed shop"), "a rescan reads the folder again");

    run(&mut h, "library.places.remove", json!({"path": shop.path()}));
    assert!(h.app.preferences.library_places.is_empty());
    assert!(places(&mut h).is_empty());
}

/// Changes in one batch all land: each one's echo gives the session its rows before the next runs.
#[test]
fn each_place_change_in_a_batch_sees_the_ones_before_it() {
    let (a, b) = (Shop::new("batch-a"), Shop::new("batch-b"));
    let mut h = Harness::new();
    let add = |shop: &Shop| json!({"command": "library.places.add", "params": {"path": shop.path(), "kind": "library"}});
    // The engine checks the second step against the place the first one added.
    let set = json!({"command": "library.places.set", "params": {"path": a.path(), "read_only": true}});
    let out = h.call("engine.batch", json!({"commands": [add(&a), set, add(&b)]}));
    assert!(out["error"].is_null(), "{out}");
    let rows: Vec<_> = h.app.preferences.library_places.iter().map(|row| (row.place.path().map(str::to_string), row.read_only)).collect();
    assert_eq!(rows, [(Some(a.path()), true), (Some(b.path()), false)]);
}

/// A scan reads with the scope of whoever starts it: a narrower scope refuses the place, and the
/// place says why.
#[test]
fn a_scan_reads_only_what_its_scope_allows() {
    let (shop, other) = (Shop::new("scoped"), Shop::new("scope-root"));
    let mut h = Harness::new();
    // Set directly, so no frame outside the scope starts the scan first.
    let place = solidcraft_engine::io::library::Place::Library(shop.path());
    h.app.preferences.library_places = vec![solidcraft_engine::io::library::PlaceRow { place, read_only: false }];
    let narrow = solidcraft_engine::io::vfs::PathScope::new([other.0.clone()]);
    let ctx = h.ctx.clone();
    solidcraft_engine::io::vfs::scoped(Some(narrow), || crate::library_places::sync(&mut h.app, &ctx));
    finish(&mut h);
    let listed = places(&mut h);
    assert_eq!(listed[0]["scan"], json!("scanned"), "{listed:?}");
    assert!(listed[0]["why"].is_string(), "the place says why it can't be read: {listed:?}");
    assert!(listed[0]["libraries"].as_array().is_some_and(Vec::is_empty), "{listed:?}");
}

/// Preferences set through `ui.prefs` follow the same rules as the commands for the folder a
/// later scan reads: inside the caller's scope, and never the built-in library again.
#[test]
fn places_set_as_preferences_stay_inside_the_callers_scope() {
    let (allowed, elsewhere) = (Shop::new("prefs-allowed"), Shop::new("prefs-elsewhere"));
    let mut h = Harness::new();
    let row = |shop: &Shop| json!({"place": {"kind": "library", "path": shop.path()}, "read_only": false});
    let scope = solidcraft_engine::io::vfs::PathScope::new([allowed.0.clone()]);
    solidcraft_engine::io::vfs::scoped(Some(scope), || {
        assert!(crate::prefs::set(&mut h.app, &json!({"library_places": [row(&allowed)]})).is_ok());
        let refused = crate::prefs::set(&mut h.app, &json!({"library_places": [row(&allowed), row(&elsewhere)]}));
        assert!(refused.is_err(), "{refused:?}");
    });
    assert!(crate::prefs::set(&mut h.app, &json!({"library_places": [{"place": {"kind": "built_in"}, "read_only": true}]})).is_err());
    // A relative or empty folder would be read from wherever SolidCraft started.
    for folder in ["relative/lib", ""] {
        let row = json!({"place": {"kind": "folder", "path": folder}, "read_only": false});
        assert!(crate::prefs::set(&mut h.app, &json!({"library_places": [row]})).is_err(), "{folder:?}");
    }
    assert!(crate::prefs::set(&mut h.app, &json!({"my_library_folder": "relative/lib"})).is_err());
    assert_eq!(h.app.preferences.library_places.len(), 1);
}

/// Writes in one batch all land, and the app keeps them: each write leaves its library read again
/// in the session, so the next one finds what it wrote, and the frame after keeps it.
#[test]
fn a_library_write_is_seen_at_once_and_kept_by_the_next_frame() {
    let shop = Shop::new("write-batch");
    let mut h = Harness::new();
    run(&mut h, "library.places.add", json!({"path": shop.path(), "kind": "library"}));
    h.frame();
    finish(&mut h);
    let steel = json!({"kind": "material", "value": {"name": "Steel", "density_g_cm3": 7.85}});
    let add = json!({"command": "library.entry.add", "params": {"library": "user.write-batch", "definition": steel}});
    let out = h.call("engine.batch", json!({"commands": [add]}));
    assert!(out["error"].is_null(), "{out}");
    let listed = run(&mut h, "library.entry.list", json!({"library": "user.write-batch"}));
    let added = listed["entries"][0].clone();
    let firmer = json!({"kind": "material", "value": {"name": "Steel", "density_g_cm3": 7.9}});
    let edit = json!({"command": "library.entry.edit", "params": {"entry": added["id"], "revision": added["revision"], "definition": firmer}});
    let out = h.call("engine.batch", json!({"commands": [edit]}));
    assert!(out["error"].is_null(), "{out}");
    h.frame();
    let listed = run(&mut h, "library.entry.list", json!({"library": "user.write-batch"}));
    assert_eq!(listed["entries"].as_array().map(Vec::len), Some(1), "{listed}");
    assert_ne!(listed["entries"][0]["revision"], added["revision"], "the frame after kept the write: {listed}");
}

/// A changed-only refresh (window open and focus) keeps the scan of a place whose files are as
/// they were, the same shared value, so nothing reads it again, and scans again a place whose
/// files changed. The Refresh button's full rescan is the test above.
#[test]
fn a_changed_only_refresh_keeps_an_unchanged_place_and_reads_a_changed_one() {
    let (same, edited) = (Shop::new("check-same"), Shop::new("check-edited"));
    let mut h = Harness::new();
    for shop in [&same, &edited] {
        run(&mut h, "library.places.add", json!({"path": shop.path(), "kind": "library"}));
    }
    h.frame();
    finish(&mut h);
    let scans = |h: &Harness| h.app.session.libraries.set().places().to_vec();
    let before = scans(&h);
    assert_eq!(before.len(), 3, "the built-in library and both places");
    let echo = run(&mut h, "library.places.refresh", json!({"changed_only": true}));
    assert_eq!(echo["changed_only"], json!(true));
    h.frame();
    finish(&mut h);
    let after = scans(&h);
    assert!(before.iter().zip(&after).all(|(a, b)| std::sync::Arc::ptr_eq(a, b)), "nothing changed: every scan kept");

    edited.named("user.check-edited", "Renamed shop");
    run(&mut h, "library.places.refresh", json!({"changed_only": true}));
    h.frame();
    finish(&mut h);
    let listed = places(&mut h);
    assert_eq!(listed[1]["libraries"][0]["name"], json!("Renamed shop"), "{listed:?}");
    let again = scans(&h);
    assert!(std::sync::Arc::ptr_eq(&after[1], &again[1]), "the unchanged place keeps its scan");
    assert!(!std::sync::Arc::ptr_eq(&after[2], &again[2]));
}
