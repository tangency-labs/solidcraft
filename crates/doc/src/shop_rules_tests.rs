use serde_json::json;

use super::*;

/// A minimal valid profile object, to break one thing at a time.
fn minimal() -> Value {
    json!({
        "id": "mine", "name": "Mine",
        "rules": [{"id": "hole.min_diameter", "unit": "mm", "limits": [{
            "value": 0.5, "relative_to": "thickness",
            "sources": [{"url": "https://example.com/rules", "read": "2026-11-25", "says": "half the thickness"}]
        }]}]
    })
}

#[test]
fn the_embedded_profiles_parse_and_every_value_has_a_source() {
    assert_eq!(profile_ids(), ["generic", "pcbway", "jlccnc", "sendcutsend"]);
    for id in profile_ids() {
        let p = profile(id).unwrap();
        assert!(p.note.as_deref().is_some_and(|n| n.contains("spot check")), "{id}: says its values need a spot check");
        for rule in &p.rules {
            let urls = rule.urls();
            assert!(!urls.is_empty() && urls.iter().all(|u| u.starts_with("https://")), "{id} {}", rule.id.as_str());
        }
    }
    assert!(profile("PCBWay").is_ok(), "ids ignore case");
}

#[test]
fn an_unknown_shop_names_the_built_in_ones() {
    let e = profile("acme").unwrap_err().to_string();
    assert!(e.contains("`acme` is not a shop profile") && e.contains("generic, pcbway, jlccnc, sendcutsend"), "{e}");
}

/// Limits worked by hand: a multiple of the thickness, plus a multiple of the radius, and steps.
#[test]
fn a_limit_works_out_from_the_thickness_and_the_radius() {
    // 2 T + R for T = 2 mm, R = 1 mm: 4 + 1 = 5.
    let pcb = profile("pcbway").unwrap().rule(RuleId::HoleToBend).unwrap();
    let needs: Vec<f64> = pcb.limits.iter().filter_map(|l| l.needed(2.0, 1.0)).collect();
    assert_eq!(needs, [4.0, 5.0]);
    // Half a die: 6 mm up to 2.29 mm of 5052, 8 mm up to 3.18, and the last step for anything thicker.
    let scs = &profile("sendcutsend").unwrap().rule(RuleId::HoleToBend).unwrap().limits[0];
    assert_eq!([2.0, 3.0, 50.0].map(|t| scs.needed(t, 0.0)), [Some(6.0), Some(8.0), Some(20.0)]);
}

#[test]
fn stock_is_the_listed_sizes_or_a_range() {
    let jlc = &profile("jlccnc").unwrap().rule(RuleId::StockThickness).unwrap().stock[0];
    assert!(jlc.admits(6.0) && jlc.admits(0.8) && !jlc.admits(7.0) && !jlc.admits(0.9));
    // 0.063 in is 1.6002 mm.
    let scs = &profile("sendcutsend").unwrap().rule(RuleId::StockThickness).unwrap().stock[0];
    assert!(scs.admits(1.6) && !scs.admits(1.5));
    let pcb = &profile("pcbway").unwrap().rule(RuleId::StockThickness).unwrap().stock[0];
    assert!(pcb.admits(2.0) && !pcb.admits(7.0) && !pcb.admits(0.2));
}

#[test]
fn a_valid_inline_profile_is_read_with_the_custom_id_when_it_has_none() {
    assert_eq!(Profile::from_value(&minimal()).unwrap().id, "mine");
    let mut v = minimal();
    v.as_object_mut().unwrap().remove("id");
    assert_eq!(Profile::from_value(&v).unwrap().id, "custom");
}

#[test]
fn a_malformed_profile_is_refused_with_the_reason_and_never_panics() {
    let broken = |edit: &dyn Fn(&mut Value)| {
        let mut v = minimal();
        edit(&mut v);
        Profile::from_value(&v).unwrap_err().to_string()
    };
    for value in [json!(0.0), json!(-1.0), json!(1e9), json!("NaN"), json!(null)] {
        let mut v = minimal();
        v["rules"][0]["limits"][0]["value"] = value.clone();
        assert!(Profile::from_value(&v).is_err(), "{value}");
    }
    assert!(broken(&|v| v["rules"][0]["limits"][0]["sources"] = json!([])).contains("give 1 to"));
    assert!(broken(&|v| v["rules"][0]["limits"][0]["sources"][0]["url"] = json!("http://x")).contains("https://"));
    assert!(broken(&|v| v["id"] = json!("Bad Id")).contains("lower case"));
    assert!(broken(&|v| v["rules"][0]["colour"] = json!(1)).contains("unknown field"));
    assert!(broken(&|v| v["rules"][0]["id"] = json!("hole.max_diameter")).contains("hole.max_diameter"));
    assert!(broken(&|v| v["rules"][0]["wanted"] = json!("plain")).contains("wanted"));
    assert!(broken(&|v| v["rules"][0]["unit"] = json!("in")).contains("mm"));
    assert!(broken(&|v| v["rules"] = json!([])).contains("1 to"));
    let twice = json!([minimal()["rules"][0].clone(), minimal()["rules"][0].clone()]);
    assert!(broken(&|v| v["rules"] = twice.clone()).contains("given twice"));
    let unsorted = json!([{"up_to_mm": 3.0, "value": 1.0}, {"up_to_mm": 2.0, "value": 2.0}]);
    assert!(
        broken(&|v| {
            v["rules"][0]["limits"][0].as_object_mut().unwrap().remove("value");
            v["rules"][0]["limits"][0].as_object_mut().unwrap().remove("relative_to");
            v["rules"][0]["limits"][0]["steps"] = unsorted.clone();
        })
        .contains("go up")
    );
    assert!(Profile::from_bytes(b"{not json").is_err());
    assert!(Profile::from_bytes(&vec![b' '; MAX_PROFILE_BYTES + 1]).is_err());
    assert!(read_file(br#"{"format": "other", "version": 1, "profiles": []}"#).is_err());
    assert!(read_file(br#"{"format": "solidcraft-shop-rules", "version": 2, "profiles": []}"#).is_err());
}

/// The file rules take sources, and `bend_lines` belongs to `file.dxf_bend_lines` alone, which needs it.
#[test]
fn the_dxf_rules_take_sources_and_bend_lines_belongs_to_one_of_them() {
    let source = json!([{"url": "https://example.com/rules", "read": "2026-11-25", "says": "a flat file has them"}]);
    let with = |rule: Value| {
        let mut v = minimal();
        v["rules"] = json!([rule]);
        Profile::from_value(&v)
    };
    assert!(with(json!({"id": "file.dxf_bend_lines", "bend_lines": "present", "sources": source.clone()})).is_ok());
    assert!(with(json!({"id": "file.dxf_bend_lines", "bend_lines": "absent", "sources": source.clone()})).is_ok());
    assert!(with(json!({"id": "file.dxf_single_layer", "sources": source.clone()})).is_ok());
    assert!(with(json!({"id": "file.dxf_closed_contours", "sources": source.clone()})).is_ok());
    assert!(with(json!({"id": "file.text_outlined", "sources": source.clone()})).is_ok());
    let e = with(json!({"id": "file.dxf_bend_lines", "sources": source.clone()})).unwrap_err().to_string();
    assert!(e.contains("bend_lines"), "{e}");
    let e = with(json!({"id": "file.dxf_single_layer", "bend_lines": "present", "sources": source.clone()})).unwrap_err().to_string();
    assert!(e.contains("bend_lines"), "{e}");
    assert!(with(json!({"id": "file.dxf_bend_lines", "bend_lines": "sometimes", "sources": source.clone()})).is_err());
    assert!(with(json!({"id": "file.text_outlined"})).is_err(), "a rule without a source is refused");
    // `from` belongs to the bend rules: a flange's minimum takes it, a hole's diameter does not.
    let limit = |from: Value| json!({"value": 2.0, "relative_to": "thickness", "from": from, "sources": source.clone()});
    assert!(with(json!({"id": "flange.min", "limits": [limit(json!("bend_line"))]})).is_ok());
    assert!(with(json!({"id": "hole.min_diameter", "limits": [limit(json!("bend_line"))]})).is_err());
}

/// Which built-in profiles ask for what of a flat DXF, as the research of 2026-11-25 read them.
#[test]
fn the_built_in_profiles_name_the_dxf_rules_their_shops_state() {
    let has = |shop: &str, id: RuleId| profile(shop).unwrap().rule(id).is_some();
    for id in [RuleId::DxfSingleLayer, RuleId::DxfClosedContours, RuleId::TextOutlined] {
        assert!(has("generic", id) && has("pcbway", id) && has("sendcutsend", id), "{}", id.as_str());
        assert!(!has("jlccnc", id), "JLCCNC's pages state no flat file rule: {}", id.as_str());
    }
    let scs = profile("sendcutsend").unwrap().rule(RuleId::DxfBendLines).unwrap();
    assert_eq!(scs.bend_lines, Some(BendLines::Present));
    assert!(!has("pcbway", RuleId::DxfBendLines) && !has("generic", RuleId::DxfBendLines), "only SendCutSend states bend lines");
}
