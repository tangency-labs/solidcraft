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

fn source(says: &str) -> Value {
    json!({"url": "https://example.com/rules", "read": "2026-11-25", "says": says})
}

/// A numeric rule's limit: `value` mm, or a multiple of the thickness, with a page.
fn limit(value: f64, times_thickness: bool) -> Value {
    if times_thickness {
        json!({"value": value, "relative_to": "thickness", "sources": [source("a multiple of the thickness")]})
    } else {
        json!({"value": value, "sources": [source("a plain length")]})
    }
}

/// A limit that is a table: `(up to this thickness, value)` rows, for some materials, measured from
/// the bend line.
fn table(materials: &[&str], rows: &[(f64, f64)]) -> Value {
    let steps: Vec<Value> = rows.iter().map(|(up_to, value)| json!({"up_to_mm": up_to, "value": value})).collect();
    json!({"materials": materials, "steps": steps, "from": "bend_line", "sources": [source("a table by thickness")]})
}

fn with(mut limit: Value, materials: &[&str]) -> Value {
    limit["materials"] = json!(materials);
    limit
}

fn rule(id: &str, extra: Value) -> Value {
    let mut r = json!({"id": id});
    if let (Some(r), Some(extra)) = (r.as_object_mut(), extra.as_object()) {
        r.extend(extra.clone());
    }
    r
}

fn plain(id: &str) -> Value {
    rule(id, json!({"sources": [source("the file rule")]}))
}

fn dxf(layers: &str, bend_lines: bool) -> Value {
    json!({"layers": layers, "bend_lines": bend_lines, "sources": [source("the export the shop asks for")]})
}

/// The synthetic shops the tests hold a part against; none is a real shop, and each has what a
/// test needs, so a test says what it measures:
///
/// - `shop_a`: a flat sheet up to 3000 x 1500 mm, smaller by material (copper, aluminium); a
///   stock range 0.5 to 6 mm whose pages disagree; hole to bend `2 T` and `2 T + R`, flange `4 T`
///   and `2.5 T` (both disagreeing); the single-layer, closed-contour and outline rules; asks for
///   one layer and no bend lines.
/// - `shop_b`: listed stock sizes (0.8 to 6 mm); a flat at most 800 mm long; hole diameter `0.5 T`
///   and `1 T`; edge `1.5 T`; flange `3 T`; hardware modelled; no thread, one-body or file rules;
///   asks for the export's defaults (a layer per role, bend lines).
/// - `shop_c`: stock by material; hole to bend and flange from tables by thickness, measured from the
///   bend line, ending at 6.35 mm; per-material hole diameter and edge; threads plain, hardware
///   named on the order; every file rule, including bend lines; asks for one cut layer and the
///   bend lines apart.
pub(crate) fn shop_object(id: &str) -> Value {
    let (name, note, dxf_export, rules) = match id {
        "shop_a" => (
            "Shop A",
            "A synthetic shop for tests.",
            Some(dxf("single", false)),
            vec![
                plain("file.one_body"),
                rule(
                    "stock.thickness",
                    json!({"disagreement": true, "stock": [{"min_mm": 0.5, "max_mm": 6.0, "sources": [source("the sheets it cuts")]}]}),
                ),
                rule(
                    "size.max_flat",
                    json!({"limits": [
                        {"value": 3000.0, "short_value": 1500.0, "sources": [source("the raw sheet")]},
                        {"materials": ["copper"], "value": 1490.0, "short_value": 590.0, "sources": [source("a smaller sheet")]},
                        {"materials": ["aluminium", "titanium"], "value": 2430.0, "short_value": 1210.0, "sources": [source("another smaller sheet")]},
                    ]}),
                ),
                rule("hole.min_diameter", json!({"unit": "mm", "process": "laser cutting", "limits": [limit(0.5, true)]})),
                rule(
                    "hole.to_bend",
                    json!({"disagreement": true, "limits": [limit(2.0, true), json!({"value": 2.0, "plus_radius": 1.0, "relative_to": "thickness", "sources": [source("plus the radius")]})]}),
                ),
                rule("flange.min", json!({"disagreement": true, "limits": [limit(4.0, true), limit(2.5, true)]})),
                plain("file.dxf_single_layer"),
                plain("file.dxf_closed_contours"),
                plain("file.text_outlined"),
            ],
        ),
        "shop_b" => (
            "Shop B",
            "A synthetic shop for tests.",
            Some(dxf("roles", true)),
            vec![
                rule(
                    "stock.thickness",
                    json!({"stock": [{"thicknesses_mm": [0.8, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0], "sources": [source("the sizes it stocks")]}]}),
                ),
                plain("file.uniform_thickness"),
                rule("size.max_flat", json!({"limits": [limit(800.0, false)]})),
                rule("hole.min_diameter", json!({"disagreement": true, "limits": [limit(0.5, true), limit(1.0, true)]})),
                rule("hole.to_edge", json!({"limits": [limit(1.5, true)]})),
                rule("flange.min", json!({"limits": [limit(3.0, true)]})),
                rule("hardware.modelling", json!({"wanted": "modelled", "sources": [source("hardware is modelled")]})),
            ],
        ),
        "shop_c" => (
            "Shop C",
            "A synthetic shop for tests.",
            Some(dxf("cut", true)),
            vec![
                plain("file.one_body"),
                plain("file.uniform_thickness"),
                rule(
                    "stock.thickness",
                    json!({"stock": [
                        {"materials": ["5052"], "thicknesses_mm": [1.6, 2.032, 3.175], "sources": [source("aluminium sizes")]},
                        {"materials": ["304"], "thicknesses_mm": [1.88, 2.54], "sources": [source("stainless sizes")]},
                    ]}),
                ),
                rule("hole.min_diameter", json!({"unit": "mm", "limits": [limit(0.5, true), with(limit(1.0, true), &["5052"])]})),
                rule("hole.to_edge", json!({"unit": "mm", "limits": [with(limit(2.0, true), &["5052"])]})),
                rule(
                    "hole.to_bend",
                    json!({"unit": "mm", "limits": [
                        table(&["5052"], &[(2.29, 6.0), (3.18, 8.0), (6.35, 20.0)]),
                        table(&["mild steel"], &[(1.9, 5.0), (3.0, 8.0), (6.35, 20.0)]),
                    ]}),
                ),
                rule("flange.min", json!({"unit": "mm", "limits": [table(&["5052"], &[(2.29, 8.0), (3.18, 10.0), (6.35, 24.0)])]})),
                rule("thread.modelling", json!({"wanted": "plain", "sources": [source("a plain hole at the tap size")]})),
                rule("hardware.modelling", json!({"wanted": "plain", "sources": [source("hardware named on the order")]})),
                plain("file.dxf_single_layer"),
                plain("file.dxf_closed_contours"),
                rule("file.dxf_bend_lines", json!({"bend_lines": "present", "sources": [source("a line at each bend")]})),
                plain("file.text_outlined"),
            ],
        ),
        _ => ("", "", None, Vec::new()),
    };
    let mut profile = json!({"id": id, "name": name, "note": note, "rules": rules});
    if let Some(d) = dxf_export {
        profile["dxf_export"] = d;
    }
    profile
}

/// A synthetic shop, parsed with the parser every profile goes through.
pub(crate) fn shop(id: &str) -> Profile {
    match Profile::from_value(&shop_object(id)) {
        Ok(p) => p,
        Err(e) => panic!("the synthetic {id}: {e}"),
    }
}

#[test]
fn the_embedded_profiles_parse_and_every_value_has_a_basis() {
    assert_eq!(profile_ids(), ["generic"], "core ships the one general profile; a shop's own rules are a preset or a plugin's");
    for id in profile_ids() {
        let p = profile(id).unwrap();
        assert!(p.note.as_deref().is_some_and(|n| n.contains("not any shop's rules")), "{id}: says it is no shop's rules");
        // Generic cites general practice as text: every basis is stated, and none is a page.
        for rule in &p.rules {
            let all = rule.sources.iter().chain(rule.limits.iter().flat_map(|l| &l.sources));
            let mut n = 0;
            for s in all {
                n += 1;
                assert!(s.says.starts_with("general practice:") && s.url.is_none() && s.read.is_none(), "{id} {}", rule.id.as_str());
            }
            assert!(n > 0 && rule.urls().is_empty(), "{id} {}", rule.id.as_str());
        }
    }
    assert!(profile("GENERIC").is_ok(), "ids ignore case");
}

#[test]
fn an_unknown_shop_names_the_built_in_ones() {
    let e = profile("acme").unwrap_err().to_string();
    assert!(e.contains("`acme` is not a shop profile") && e.contains("the built-in one is generic"), "{e}");
}

/// Limits worked by hand: a multiple of the thickness, plus a multiple of the radius, and steps.
#[test]
fn a_limit_works_out_from_the_thickness_and_the_radius() {
    // 2 T + R for T = 2 mm, R = 1 mm: 4 + 1 = 5.
    let a = shop("shop_a");
    let a = a.rule(RuleId::HoleToBend).unwrap();
    let needs: Vec<f64> = a.limits.iter().filter_map(|l| l.needed(2.0, 1.0)).collect();
    assert_eq!(needs, [4.0, 5.0]);
    // Half a die: 6 mm up to 2.29 mm of 5052, 8 mm up to 3.18, 20 mm up to the table's end at 6.35;
    // a thicker sheet is past the table and has no value (the last step is not carried on).
    let tabled = shop("shop_c");
    let tabled = &tabled.rule(RuleId::HoleToBend).unwrap().limits[0];
    assert_eq!([2.0, 3.0, 6.35, 9.525, 50.0].map(|t| tabled.needed(t, 0.0)), [Some(6.0), Some(8.0), Some(20.0), None, None]);
    assert_eq!(tabled.table_end(), Some(6.35));
}

#[test]
fn stock_is_the_listed_sizes_or_a_range() {
    let listed = shop("shop_b");
    let listed = &listed.rule(RuleId::StockThickness).unwrap().stock[0];
    assert!(listed.admits(6.0) && listed.admits(0.8) && !listed.admits(7.0) && !listed.admits(0.9));
    // 0.063 in is 1.6002 mm.
    let tabled = shop("shop_c");
    let tabled = &tabled.rule(RuleId::StockThickness).unwrap().stock[0];
    assert!(tabled.admits(1.6) && !tabled.admits(1.5));
    let a = shop("shop_a");
    let a = &a.rule(RuleId::StockThickness).unwrap().stock[0];
    assert!(a.admits(2.0) && !a.admits(7.0) && !a.admits(0.2));
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
    for url in
        ["https://", "https:///page", "https://.example.com", "https://-x.com/", "https://x.com:/a", "https://x.com:99999999/a", "https://a b.com"]
    {
        assert!(broken(&|v| v["rules"][0]["limits"][0]["sources"][0]["url"] = json!(url)).contains("https://"), "{url}");
    }
    let ok = |url: &str| {
        let mut v = minimal();
        v["rules"][0]["limits"][0]["sources"][0]["url"] = json!(url);
        Profile::from_value(&v).is_ok()
    };
    assert!(ok("https://example.com") && ok("https://www.example.com/a/b.html?x=1#y") && ok("https://example.com:8443/a"));
    // A basis stated as text needs no page; it still has to say something.
    let mut v = minimal();
    v["rules"][0]["limits"][0]["sources"] = json!([{"says": "general practice: half the thickness"}]);
    assert!(Profile::from_value(&v).is_ok());
    assert!(broken(&|v| v["rules"][0]["limits"][0]["sources"] = json!([{"url": "https://example.com"}])).contains("sources"));
    assert!(broken(&|v| v["rules"][0]["limits"][0]["sources"] = json!([{"says": " "}])).contains("says"));
    // A bend radius is only there for the bend rules: a formula with it elsewhere would be dropped.
    assert!(broken(&|v| v["rules"][0]["limits"][0]["plus_radius"] = json!(1.0)).contains("plus_radius"));
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

/// A built-in or synthetic profile by id.
pub(crate) fn any(id: &str) -> Profile {
    match profile(id) {
        Ok(p) => p.clone(),
        Err(_) => shop(id),
    }
}

/// Which profiles ask for what of a flat DXF: Generic (a strict profile) and the synthetic shops.
#[test]
fn the_dxf_rules_of_a_profile_are_the_ones_it_names() {
    let has = |shop: &str, id: RuleId| any(shop).rule(id).is_some();
    for id in [RuleId::DxfSingleLayer, RuleId::DxfClosedContours, RuleId::TextOutlined] {
        assert!(has("generic", id) && has("shop_a", id) && has("shop_c", id), "{}", id.as_str());
        assert!(!has("shop_b", id), "shop_b states no flat file rule: {}", id.as_str());
    }
    let c = any("shop_c");
    assert_eq!(c.rule(RuleId::DxfBendLines).unwrap().bend_lines, Some(BendLines::Present));
    assert!(!has("shop_a", RuleId::DxfBendLines) && !has("generic", RuleId::DxfBendLines), "only shop_c states bend lines");
}

/// Each profile names the export it wants, with the pages it is read from. Generic's recommendation is
/// a cutter-safe file, which no preset need copy.
#[test]
fn each_profile_recommends_the_export_it_wants() {
    let rec = |shop: &str| any(shop).dxf_export.unwrap_or_else(|| panic!("{shop} names an export"));
    for id in ["generic", "shop_a", "shop_b", "shop_c"] {
        let r = rec(id);
        // The synthetic shops cite pages; Generic states a basis as text.
        let pages = id != "generic";
        assert!(!r.sources.is_empty() && r.sources.iter().all(|s| s.url.as_deref().is_some_and(|u| u.starts_with("https://")) == pages), "{id}");
        assert!(!r.bend_notes && !r.hardware_notes, "{id}: notes are text entities, which a file rule may refuse");
    }
    // A cutter-safe file: one layer and no bend lines for a shop that cuts every line.
    for shop in ["generic", "shop_a"] {
        let r = rec(shop);
        assert_eq!((r.layers, r.bend_lines), (DxfLayers::Single, false), "{shop}");
    }
    // One cut layer, and the bend lines apart from it.
    let c = rec("shop_c");
    assert_eq!((c.layers, c.bend_lines), (DxfLayers::Cut, true));
    // The export's own defaults.
    let b = rec("shop_b");
    assert_eq!((b.layers, b.bend_lines), (DxfLayers::Roles, true));
}

/// `dxf_export` is read with the export's defaults for what it leaves out, and is checked.
#[test]
fn a_dxf_export_is_read_and_checked() {
    let source = json!([{"url": "https://example.com/rules", "read": "2026-11-25", "says": "one cut layer"}]);
    let with = |dxf: Value| {
        let mut v = minimal();
        v["dxf_export"] = dxf;
        Profile::from_value(&v)
    };
    let p = with(json!({"sources": source.clone()})).unwrap().dxf_export.unwrap();
    assert_eq!((p.layers, p.bend_lines, p.bend_notes, p.hardware_notes), (DxfLayers::Roles, true, false, false));
    let p = with(json!({"layers": "cut", "bend_lines": false, "bend_notes": true, "hardware_notes": true, "sources": source.clone()})).unwrap();
    assert_eq!(p.dxf_export.map(|d| (d.layers, d.bend_lines)), Some((DxfLayers::Cut, false)));
    assert!(Profile::from_value(&minimal()).unwrap().dxf_export.is_none(), "optional");
    assert!(with(json!({"layers": "single"})).is_err(), "a recommendation names its page");
    assert!(with(json!({"layers": "single", "sources": []})).is_err());
    assert!(with(json!({"layers": "many", "sources": source.clone()})).is_err());
    assert!(with(json!({"colour": 1, "sources": source.clone()})).unwrap_err().to_string().contains("unknown field"));
    let e = with(json!({"layers": "single", "hardware_notes": true, "sources": source})).unwrap_err().to_string();
    assert!(e.contains("hardware_notes"), "{e}");
}
