use serde_json::json;

use super::*;
use crate::shop_rules::tests::shop_object;

#[test]
fn a_name_makes_an_id_of_letters_digits_and_single_dashes() {
    assert_eq!(slug("My Shop (laser)").as_deref(), Some("my-shop-laser"));
    assert_eq!(slug("  A  B ").as_deref(), Some("a-b"));
    assert_eq!(slug("Shop 42").as_deref(), Some("shop-42"));
    assert_eq!(slug("???"), None);
    assert_eq!(slug("").as_deref(), None);
    let long = slug(&"abc ".repeat(20)).unwrap_or_default();
    assert!(long.len() <= 32 && !long.ends_with('-'), "{long}");
}

#[test]
fn a_preset_is_a_checked_profile_under_a_name_and_an_id() {
    // The id is the profile's own when it has one, and made from the name when it has none.
    let p = Preset::new(Some("Corner Shop"), shop_object("shop_a")).unwrap();
    assert_eq!((p.id.as_str(), p.name.as_str()), ("shop_a", "Corner Shop"));
    assert_eq!(p.profile["name"], "Corner Shop", "the name given is the profile's");
    assert_eq!(p.parsed().unwrap().name, "Corner Shop");
    let mut object = shop_object("shop_a");
    object.as_object_mut().unwrap().remove("id");
    assert_eq!(Preset::new(Some("Corner Shop"), object.clone()).unwrap().id, "corner-shop");
    // Without a name, the profile's own.
    assert_eq!(Preset::new(None, object).unwrap().name, "Shop A");
    // Generic's values, as the file writes them, are a start for a preset of its own.
    let generic = shop_rules::profile_value("generic").unwrap();
    let mine = Preset::new(Some("Mine"), generic).unwrap();
    assert_eq!(mine.id, "mine");
    assert_eq!(mine.parsed().unwrap().rules.len(), shop_rules::profile("generic").unwrap().rules.len());
}

#[test]
fn a_name_that_clashes_with_the_built_in_profile_is_refused() {
    let generic = shop_rules::profile_value("generic").unwrap();
    for (name, id) in [("Generic", None), ("generic", None), ("My shop", Some("generic")), ("My shop", Some("GENERIC"))] {
        let mut object = generic.clone();
        match id {
            Some(id) => object["id"] = json!(id),
            None => {
                object.as_object_mut().unwrap().remove("id");
            }
        }
        let e = Preset::new(Some(name), object).unwrap_err().to_string();
        assert!(e.contains("`generic` is the built-in profile"), "{name} {id:?}: {e}");
    }
}

#[test]
fn bad_input_is_refused_with_the_reason_and_never_panics() {
    let refused = |name: Option<&str>, profile: serde_json::Value| Preset::new(name, profile).unwrap_err().to_string();
    assert!(refused(Some("X"), json!("a string")).contains("an object"));
    assert!(refused(Some("X"), json!(null)).contains("an object"));
    assert!(refused(Some("X"), json!([1, 2])).contains("an object"));
    assert!(refused(Some(""), shop_object("shop_a")).contains("`name`"));
    assert!(refused(Some("   "), shop_object("shop_a")).contains("`name`"));
    assert!(refused(Some(&"x".repeat(65)), shop_object("shop_a")).contains("`name`"));
    assert!(refused(Some("Bell\u{7}"), shop_object("shop_a")).contains("`name`"));
    assert!(refused(Some("???"), json!({"rules": []})).contains("no letter or digit"));
    assert!(refused(Some("X"), json!({"id": 5, "rules": []})).contains("`id` is text"));
    assert!(refused(Some("X"), json!({"id": "Bad Id", "rules": []})).contains("lower case"));
    // The profile itself is held to the profile rules.
    assert!(refused(Some("X"), json!({"rules": []})).contains("1 to"));
    let mut broken = shop_object("shop_a");
    broken["rules"][3]["limits"][0]["value"] = json!(-1.0);
    assert!(refused(Some("X"), broken).contains("hole.min_diameter"));
    let mut insecure = shop_object("shop_a");
    insecure["rules"][3]["limits"][0]["sources"][0]["url"] = json!("http://example.com");
    assert!(refused(Some("X"), insecure).contains("https://"));
    let mut huge = shop_object("shop_a");
    huge["note"] = json!("n".repeat(100_000));
    assert!(Preset::new(Some("X"), huge).is_err());
}

#[test]
fn presets_are_saved_listed_replaced_and_deleted() {
    let mut presets = Presets::default();
    assert!(presets.is_empty() && presets.list().is_empty());
    assert_eq!(presets.save(Preset::new(Some("Alpha"), shop_object("shop_a")).unwrap()), Ok(false));
    assert_eq!(presets.save(Preset::new(Some("Beta"), shop_object("shop_b")).unwrap()), Ok(false));
    assert_eq!(presets.list().iter().map(|p| (p.id.as_str(), p.name.as_str())).collect::<Vec<_>>(), [("shop_a", "Alpha"), ("shop_b", "Beta")]);
    assert_eq!(presets.get("SHOP_B").map(|p| p.name.as_str()), Some("Beta"), "ids ignore case");
    // The same id again replaces it, in its place.
    assert_eq!(presets.save(Preset::new(Some("Alpha 2"), shop_object("shop_a")).unwrap()), Ok(true));
    assert_eq!(presets.list().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Alpha 2", "Beta"]);
    // Another id with a name in use is refused, and nothing changes.
    let before = presets.clone();
    let mut other = shop_object("shop_c");
    other["id"] = json!("shop_d");
    let e = presets.save(Preset::new(Some("beta"), other).unwrap()).unwrap_err().to_string();
    assert!(e.contains("already has the name"), "{e}");
    assert_eq!(presets, before);
    let gone = presets.delete("shop_a").unwrap();
    assert_eq!(gone.name, "Alpha 2");
    assert_eq!(presets.list().len(), 1);
    let e = presets.delete("shop_a").unwrap_err().to_string();
    assert!(e.contains("`shop_a` is not a shop preset") && e.contains("the presets are shop_b"), "{e}");
    presets.delete("shop_b").unwrap();
    assert!(presets.delete("x").unwrap_err().to_string().contains("there are no presets saved"));
}

#[test]
fn there_is_a_most_to_the_presets_a_person_keeps() {
    let mut presets = Presets::default();
    for n in 0..MAX_PRESETS {
        let mut object = shop_object("shop_a");
        object["id"] = json!(format!("shop-{n}"));
        presets.save(Preset::new(Some(&format!("Shop number {n}")), object).unwrap()).unwrap();
    }
    let mut one_more = shop_object("shop_a");
    one_more["id"] = json!("one-more");
    let e = presets.save(Preset::new(Some("One more"), one_more).unwrap()).unwrap_err().to_string();
    assert!(e.contains("at most 32"), "{e}");
    assert_eq!(presets.list().len(), MAX_PRESETS);
    // A saved list longer than that is read back cut to it.
    let rows: Vec<serde_json::Value> = (0..MAX_PRESETS + 5)
        .map(|n| {
            let mut object = shop_object("shop_a");
            object["id"] = json!(format!("shop-{n}"));
            object["name"] = json!(format!("Shop number {n}"));
            json!({"id": format!("shop-{n}"), "name": format!("Shop number {n}"), "profile": object})
        })
        .collect();
    let read: Presets = serde_json::from_value(json!(rows)).unwrap();
    assert_eq!(read.list().len(), MAX_PRESETS);
}

/// What is stored comes back; a damaged row, a repeat and a row whose id is not its profile's are left
/// out, and the rest are kept; a stored value that is not a list is no presets.
#[test]
fn stored_presets_read_back_and_a_damaged_row_leaves_out_only_itself() {
    let mut presets = Presets::default();
    presets.save(Preset::new(Some("Alpha"), shop_object("shop_a")).unwrap()).unwrap();
    presets.save(Preset::new(Some("Beta"), shop_object("shop_b")).unwrap()).unwrap();
    let stored = serde_json::to_value(&presets).unwrap();
    assert_eq!(serde_json::from_value::<Presets>(stored.clone()).unwrap(), presets, "a round trip");
    let mut rows = stored.as_array().cloned().unwrap_or_default();
    let mut broken = rows[0].clone();
    broken["profile"]["rules"] = json!([]);
    let mut renamed = rows[1].clone();
    renamed["id"] = json!("not-the-profiles");
    let mut with_generic = rows[0].clone();
    with_generic["id"] = json!("generic");
    with_generic["profile"]["id"] = json!("generic");
    rows.extend([json!(null), json!(7), json!({"id": "x"}), broken, renamed, with_generic, rows[0].clone()]);
    let read: Presets = serde_json::from_value(json!(rows)).unwrap();
    assert_eq!(read, presets, "only the two good rows");
    for value in [json!(null), json!({"shop_a": 1}), json!("text"), json!(3)] {
        assert!(serde_json::from_value::<Presets>(value).unwrap().is_empty());
    }
}
