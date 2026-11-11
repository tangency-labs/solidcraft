//! SolidCraft's theme files through the kit's `theme_files`: its shipped themes, read against its
//! tab table and icon palette.
use crate::test_support::{is_two_colour, tab_fixture};
use crate::theme_files::fixtures::*;
use crate::theme_files::*;
use egui::Color32;
use serde_json::{Value, json};
use solidcraft_theme::Tokens;
use std::collections::BTreeMap;

#[test]
fn tabs_import_is_strict_and_roundtrips_authored_surfaces() {
    crate::test_support::host_data();
    for base in ["dark", "light"] {
        let file = tab_fixture(base);
        assert_eq!(from_value(&export_file(&file)).unwrap(), file);
        let mut source = export_file(&file);
        source["tabs"] = json!(crate::workspace::TAB_ACCENTS.names().map(|tab| (tab, json!({"panel":"#18202aff"}))).collect::<BTreeMap<_, _>>());
        assert_eq!(from_value(&source).unwrap().tabs.len(), crate::workspace::TAB_ACCENTS.names().len());
        for (tabs, offending) in [
            (json!(null), "tabs"),
            (json!([]), "tabs"),
            (json!({"sketch":{}}), "sketch"),
            (json!({"MISSING":{}}), "MISSING"),
            (json!({"SKETCH":null}), "SKETCH"),
            (json!({"SKETCH":{"panel":"#bad"}}), "panel"),
            (json!({"SKETCH":{"panel":["#ffffff"]}}), "panel"),
            (json!({"SKETCH":{"panel":12}}), "panel"),
        ] {
            source["tabs"] = tabs;
            assert!(from_value(&source).unwrap_err().contains(offending));
        }
        // Refuse every known non-surface token, not only status colours.
        for key in keys().into_iter().filter(|key| !matches!(*key, "panel" | "viewport_top" | "viewport_bottom")) {
            source["tabs"] = json!({"SKETCH":{key:"#ffffff"}});
            assert!(from_value(&source).unwrap_err().contains(key), "{key}");
        }
        source["tabs"] = json!({"SKETCH":{"viewport_top":"#12345678"}});
        let file = from_value(&source).unwrap();
        assert_eq!(export_file(&file)["tabs"], source["tabs"]);
    }
}

#[test]
fn tabs_resolve_only_surfaces_and_off_is_exactly_base() {
    crate::test_support::host_data();
    for base in ["dark", "light"] {
        let file = tab_fixture(base);
        let defaults = BTreeMap::new();
        let base = resolve_with(&file, &defaults);
        for tab in crate::workspace::TAB_ACCENTS.names() {
            let mut want = base;
            if let Some(surfaces) = file.tabs.get(tab) {
                for (key, value) in surfaces {
                    slot(&mut want, key).unwrap().copy_from_slice(&colors_of(value, 1).unwrap());
                }
            }
            let effective = resolve_for_tab(&file, &defaults, Some(tab));
            assert_eq!(effective, want, "{tab}");
            assert_eq!(crate::workspace::tab_accent(&effective, tab), crate::workspace::tab_accent(&base, tab));
            let off = BTreeMap::from([("tab_colours".into(), OptionValue::Bool(false))]);
            assert_eq!(resolve_for_tab(&file, &off, Some(tab)), resolve_with(&file, &off));
        }
        let mut damaged = file.clone();
        damaged.tabs.get_mut("SKETCH").unwrap().insert("error".into(), json!("#ffffff"));
        assert_eq!(resolve_for_tab(&damaged, &defaults, Some("SKETCH")), base);
        let mut absent = file.clone();
        absent.tabs.clear();
        assert_eq!(resolve_for_tab(&absent, &defaults, Some("SKETCH")), base);
        assert_eq!(export_file(&file)["options"][1]["default"], 240.0);
    }
    for file in builtin() {
        let off = BTreeMap::from([("tab_colours".into(), OptionValue::Bool(false))]);
        let mut without_tabs = file.clone();
        without_tabs.tabs.clear();
        for tab in crate::workspace::TAB_ACCENTS.names() {
            assert_eq!(resolve_for_tab(file, &off, Some(tab)), resolve_with(file, &off), "{} {tab}: tabs off", file.id);
            assert_eq!(resolve_for_tab(&without_tabs, &BTreeMap::new(), Some(tab)), resolve(file), "{} {tab}: no tables", file.id);
            assert_eq!(
                resolve_for_tab(file, &BTreeMap::new(), Some(tab)),
                resolve_for_tab(&without_tabs, &BTreeMap::new(), Some(tab)),
                "{} {tab}: authored defaults leave tab surfaces off",
                file.id
            );
        }
    }
}

#[test]
fn reserved_options_are_app_owned_and_ordered_with_their_children() {
    crate::test_support::host_data();
    let file = from_value(&option_theme(
        json!([
            {"id":"custom_first","type":"bool","default":false},
            {"id":"tab_fade_ms","type":"range","default":9999,"min":100,"max":500,"unit":"bad","parent":"wrong","step":null},
            {"id":"tab_colours","type":"bool","default":true,"advanced":"ignored","label":null},
            {"id":"blur","type":"range","default":-5,"help":false,"section":42},
            {"id":"custom_child","type":"choice","default":"a","parent":"tab_fade_ms",
                "values":[{"id":"a","label":{}},{"id":"b","label":{}}]},
            {"id":"opacity","type":"range","default":200,"min":60,"max":90,"step":0,"unit":false}
        ]),
        json!([]),
    ))
    .unwrap_err();
    assert!(file.contains("parent"), "a reserved range cannot be a bool parent");
    let file = from_value(&option_theme(
        json!([
            {"id":"custom_first","type":"bool","default":false},
            {"id":"tab_fade_ms","type":"range","default":9999,"min":100,"max":500,"unit":"bad","parent":"wrong","step":null},
            {"id":"tab_colours","type":"bool","default":true,"advanced":"ignored","label":null},
            {"id":"blur","type":"range","default":-5,"help":false,"section":42},
            {"id":"custom_child","type":"bool","default":true,"parent":"custom_first"},
            {"id":"opacity","type":"range","default":200,"min":60,"max":90,"step":0,"unit":false}
        ]),
        json!([]),
    ))
    .unwrap();
    let (values, notices) = effective_values(&file, &BTreeMap::new());
    assert!(notices.is_empty());
    assert_eq!(values["opacity"], OptionValue::Num(90.0));
    assert_eq!(values["blur"], OptionValue::Num(0.0));
    assert_eq!(values["tab_fade_ms"], OptionValue::Num(500.0));
    let fade = file.options.iter().find(|o| o.id == "tab_fade_ms").unwrap();
    assert_eq!(fade.parent.as_deref(), Some("tab_colours"));
    assert!(matches!(&fade.kind, OptionKind::Range { min:100.0, max:500.0, step:20.0, unit:Some(unit), .. } if unit == "ms"));
    assert!(
        file.options
            .iter()
            .filter(|o| STANDARD_OPTIONS.contains(&o.id.as_str()))
            .all(|o| o.label.is_empty() && o.help.is_empty() && !o.advanced && o.section.is_none())
    );
    assert_eq!(
        ordered_options(&file).iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["opacity", "blur", "tab_colours", "tab_fade_ms", "custom_first", "custom_child"]
    );
    let child = from_value(&option_theme(
        json!([
            {"id":"child","type":"bool","default":false,"parent":"tab_colours"},
            {"id":"other","type":"bool","default":false},
            {"id":"tab_colours","type":"bool","default":true}
        ]),
        json!([]),
    ))
    .unwrap();
    assert_eq!(ordered_options(&child).iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["tab_colours", "child", "other"]);
}

#[test]
fn matching_overlays_apply_in_file_order_and_preserve_other_table_entries() {
    crate::test_support::host_data();
    let mut source = option_theme(
        sample_options(),
        json!([
            {"when":{"opt":"enabled","eq":true},"colors":{"panel":"#123456","axis_x":"#111111"},"radii":{"radius_sm":1},
                "borders":{"primary":{"width":2,"color":"#abcdef"}},"icon_palette":{"ink":"#112233"}},
            {"when":{"opt":"amount","gte":5,"lte":5},"colors":{"panel":"#abcdef","triad":["#222222","#333333","#444444"]},
                "radii":{"radius_md":2},"icon_palette":{"line":"#445566"}},
            {"when":{"opt":"amount","eq":6},"colors":{"panel":"#000000"}},
            {"when":{"opt":"style","eq":"cool"},"colors":{"panel":"#ffffff"}}
        ]),
    );
    source["colors"] = json!({"text":"#fedcba","axis_x":"#000000"});
    source["radii"] = json!({"radius_lg":3});
    source["borders"] = json!({"control":{"width":1,"color":"#123456"}});
    source["icon_palette"] = json!({"solid":["#ffffff","#777777","#000000"]});
    let theme = from_value(&source).unwrap();
    let resolved = resolve(&theme);
    assert_eq!(resolved.panel, parse_color("#abcdef").unwrap());
    assert_eq!(resolved.text, parse_color("#fedcba").unwrap());
    assert_eq!(resolved.axes(), [parse_color("#222222").unwrap(), parse_color("#333333").unwrap(), parse_color("#444444").unwrap()]);
    assert_eq!((resolved.radius_sm, resolved.radius_md, resolved.radius_lg), (1, 2, 3));
    assert_eq!(resolved.borders.primary.unwrap().width, 2.0);
    assert_eq!(resolved.borders.control.unwrap().width, 1.0);
    let palette = export_icon_palette(resolve_icon_palette(&theme, &BTreeMap::new()));
    assert_eq!(palette["ink"], json!("#112233"));
    assert_eq!(palette["line"], json!("#445566"));
    assert_eq!(palette["solid"], source["icon_palette"]["solid"]);
    assert_eq!(resolve_with(&theme, &BTreeMap::from([("amount".into(), OptionValue::Num(6.0))])).panel, Color32::BLACK);
    let no_matches = BTreeMap::from([("enabled".into(), OptionValue::Bool(false)), ("amount".into(), OptionValue::Num(1.0))]);
    assert_eq!(resolve_with(&theme, &no_matches), resolve_base(&theme));
    assert_eq!(resolve_with(&theme, &effective_values(&theme, &BTreeMap::new()).0), resolved);
}

#[test]
fn original_shipped_themes_keep_the_original_resolver_output_and_serialization() {
    crate::test_support::host_data();
    assert_eq!(builtin().len(), solidcraft_look::themes::BUILTIN.len());
    for id in ORIGINAL_BUILTIN_IDS {
        let theme = find(id, &[]).unwrap();
        assert!(theme.tabs.is_empty(), "{}", theme.id);
        let gruvbox = matches!(theme.id.as_str(), "gruvbox-dark" | "gruvbox-light");
        assert!(!theme.needs_newer());
        assert_eq!(!theme.options.is_empty(), gruvbox);
        assert_eq!(!theme.overlays.is_empty(), gruvbox);
        assert_eq!(resolve(theme), resolve_base(theme), "{}", theme.id);
        assert_eq!(resolve_with(theme, &BTreeMap::from([("opacity".into(), OptionValue::Num(40.0))])), resolve_base(theme), "{}", theme.id);
        let stored = serde_json::to_value(theme).unwrap();
        assert!(stored.get("requires").is_none(), "{}", theme.id);
        for key in ["options", "overlays"] {
            assert_eq!(stored.get(key).is_some(), gruvbox, "{}: {key}", theme.id);
        }
    }
}

#[test]
fn shipped_theme_defaults_and_definition_roundtrips_preserve_all_tables() {
    crate::test_support::host_data();
    assert_eq!(builtin().len(), solidcraft_look::themes::BUILTIN.len());
    for theme in builtin() {
        assert!(!theme.needs_newer(), "{}", theme.id);
        let (defaults, notices) = effective_values(theme, &BTreeMap::new());
        assert!(notices.is_empty(), "{}", theme.id);
        assert_eq!(defaults.len(), theme.options.len(), "{}: every declaration is valid", theme.id);
        assert_eq!(resolve_with(theme, &defaults), resolve(theme), "{}", theme.id);
        let stored = serde_json::to_value(theme).unwrap();
        assert_eq!(stored.get("requires").is_none(), theme.requires == 1, "{}", theme.id);
        for (key, present) in [("options", !theme.options.is_empty()), ("overlays", !theme.overlays.is_empty()), ("tabs", !theme.tabs.is_empty())] {
            assert_eq!(stored.get(key).is_some(), present, "{}: {key}", theme.id);
        }
        let restored: ThemeFile = serde_json::from_value(stored).unwrap();
        assert_eq!(&restored, theme, "{}", theme.id);
        let exported = export_file(theme);
        let mut imported = parse(&exported.to_string()).unwrap();
        // Built-in identity is assigned by the source filename, not by the exported name.
        imported.id.clone_from(&theme.id);
        assert_eq!(&imported, theme, "{}", theme.id);
        assert_eq!(resolve(&imported), resolve(theme), "{}", theme.id);
    }
}

#[test]
fn malformed_border_settings_are_refused_and_saved_settings_are_safe() {
    crate::test_support::host_data();
    for settings in [
        json!([]),
        json!({"unknown":{"width":1,"color":"#ffffff"}}),
        json!({"primary":true}),
        json!({"primary":{"width":-1,"color":"#ffffff"}}),
        json!({"primary":{"width":4.1,"color":"#ffffff"}}),
        json!({"primary":{"width":"1","color":"#ffffff"}}),
        json!({"primary":{"width":1,"color":"white"}}),
        json!({"primary":{"width":1,"color":"#ffffff","gap":1}}),
        json!({"primary_inset":{"width":1,"color":"#ffffff","gap":13}}),
        json!({"primary_inset":{"width":1,"color":"#ffffff"}}),
    ] {
        assert!(from_value(&json!({"format":FORMAT,"name":"Bad","base":"dark","borders":settings})).is_err(), "{settings}");
    }
    let file = ThemeFile { borders: BTreeMap::from([("primary".into(), json!({"width":1e100,"color":"#ffffff"}))]), ..Default::default() };
    assert_eq!(resolve(&file).borders, solidcraft_theme::Borders::LEGACY);
    assert!(parse_borders(&BTreeMap::from([("primary".into(), json!({"width":null,"color":"#ffffff"}))])).is_err());
    assert_eq!(resolve(find("blueprint", &[]).unwrap()).borders.primary.unwrap().color, Color32::WHITE);
    for id in ["gruvbox-dark", "gruvbox-light"] {
        assert_eq!(resolve(find(id, &[]).unwrap()).borders, solidcraft_theme::Borders::LEGACY);
    }
}

#[test]
fn bevel_settings_roundtrip_and_reject_invalid_values() {
    crate::test_support::host_data();
    let bevel = json!({"width":1,"highlight":"#ffffff","light":"#dfdfdf","shadow":"#808080","dark_shadow":"#000000"});
    for base in ["light", "dark"] {
        let source = json!({"format":FORMAT,"name":"Beveled","base":base,"borders":{"bevel":bevel}});
        let file = from_value(&source).unwrap();
        let tokens = resolve(&file);
        assert_eq!(resolve(&from_value(&export("Beveled", file.base, &tokens, resolve_icon_palette(&file, &BTreeMap::new()))).unwrap()), tokens);
        assert_eq!(tokens.input_fill(), tokens.field);
        for invalid in [json!(0), json!(-1), json!(2.1), json!("1"), Value::Null] {
            let mut bad = source.clone();
            bad["borders"]["bevel"]["width"] = invalid;
            assert!(from_value(&bad).is_err());
        }
        for key in ["highlight", "light", "shadow", "dark_shadow"] {
            let mut bad = source.clone();
            bad["borders"]["bevel"][key] = json!("white");
            assert!(from_value(&bad).is_err());
        }
        let mut bad = source;
        bad["borders"]["bevel"]["extra"] = json!(true);
        assert!(from_value(&bad).is_err());
    }
    for theme in builtin() {
        assert_eq!(resolve(theme).borders.bevel.is_some(), theme.id == "solidcraft-95", "{}", theme.id);
    }
}

#[test]
fn icon_palette_import_export_is_bounded_and_keeps_legacy_defaults() {
    crate::test_support::host_data();
    let source = json!({"format":FORMAT,"name":"Icons","base":"dark","icon_palette": {
        "ink":"#123456", "line":"#abcdef", "solid":["#c0d0e0","#456789","#123456"],
        "sketch":["#c0e0d0","#458967","#125634"] }});
    let file = from_value(&source).unwrap();
    let tokens = resolve(&file);
    let icons = resolve_icon_palette(&file, &BTreeMap::new());
    assert!(icons.is_some());
    let exported = from_value(&export("Icons", file.base, &tokens, icons)).unwrap();
    assert_eq!((resolve(&exported), resolve_icon_palette(&exported, &BTreeMap::new())), (tokens, icons));
    for (key, value) in [
        ("gap", json!("#ffffff")),
        ("unknown", json!("#ffffff")),
        ("ink", json!("#ffffff80")),
        ("solid", json!(["#ffffff"])),
        ("surface", json!(["#ffffff", "#ffffff", "no"])),
    ] {
        let mut bad = source.clone();
        bad["icon_palette"][key] = value;
        assert!(from_value(&bad).is_err(), "{bad}");
    }
    let old = from_value(&json!({"format":FORMAT,"name":"Legacy","base":"light"})).unwrap();
    assert!(resolve_icon_palette(&old, &BTreeMap::new()).is_none());
    let damaged = ThemeFile { icon_palette: BTreeMap::from([("gap".into(), json!("#ffffff"))]), ..Default::default() };
    assert!(resolve_icon_palette(&damaged, &BTreeMap::new()).is_none());
}

#[test]
fn shipped_themes_have_readable_distinct_workspace_colours_and_icon_palettes() {
    crate::test_support::host_data();
    use solidcraft_theme::contrast_ratio;
    assert_eq!(builtin().len(), solidcraft_look::themes::BUILTIN.len(), "all shipped themes must be valid");
    for file in builtin() {
        let gruvbox = matches!(file.id.as_str(), "gruvbox-dark" | "gruvbox-light");
        let contrasts: &[&str] = if gruvbox { &["soft", "standard", "hard"] } else { &["standard"] };
        for contrast in contrasts {
            let options = if gruvbox { BTreeMap::from([("contrast".into(), OptionValue::Choice((*contrast).into()))]) } else { BTreeMap::new() };
            let t = resolve_with(file, &options);
            assert!(resolve_icon_palette(file, &options).is_some(), "{} {contrast} has no icon palette", file.id);
            // A two-colour theme (1-bit, 1-bit Dark) can't give each workspace its own fill; it
            // still has to keep every tab readable, below.
            let two_colour = is_two_colour(&file.id);
            let mut fills = std::collections::HashSet::new();
            for tab in ["MODEL", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER", "SKETCH"] {
                let accent = crate::workspace::tab_accent(&t, tab);
                assert!(fills.insert(accent.color) || two_colour, "{} {contrast} repeats {tab}", file.id);
                assert!(contrast_ratio(accent.ink, accent.soft) >= 4.5, "{} {contrast} {tab} soft text", file.id);
                assert!(contrast_ratio(accent.on_fill(&t), accent.color) >= 4.5, "{} {contrast} {tab} pill text", file.id);
            }
        }
    }
}

#[test]
fn one_bit_themes_are_complete_opaque_black_and_white() {
    crate::test_support::host_data();
    for (id, name, base) in [("1-bit", "1-bit", Base::Light), ("1-bit-dark", "1-bit Dark", Base::Dark)] {
        assert!(is_two_colour(id));
        let file = find(id, &[]).unwrap();
        assert_eq!(file.name, name);
        assert_eq!(file.base, base);
        one_bit_is_complete_opaque_black_and_white(file);
    }
}

#[test]
fn one_bit_selection_and_viewport_tokens_have_full_contrast() {
    crate::test_support::host_data();
    for (id, paper, ink) in [("1-bit", Color32::WHITE, Color32::BLACK), ("1-bit-dark", Color32::BLACK, Color32::WHITE)] {
        one_bit_has_full_contrast(&resolve(find(id, &[]).unwrap()), id, paper, ink);
    }
}

#[test]
fn gruvbox_contrast_choices_keep_six_appearances() {
    crate::test_support::host_data();
    let families: Vec<_> = builtin().iter().filter(|f| f.id.starts_with("gruvbox-")).map(|f| f.id.as_str()).collect();
    assert_eq!(families, ["gruvbox-dark", "gruvbox-light"]);
    for (id, panels, tops, bottoms, inks) in [
        (
            "gruvbox-dark",
            ["#32302f", "#282828", "#1d2021"],
            ["#373432", "#32302f", "#2c2c2c"],
            ["#373432", "#32302f", "#2c2c2c"],
            ["#1d2021", "#1d2021", "#141617"],
        ),
        ("gruvbox-light", ["#f2e5bc", "#fbf1c7", "#f9f5d7"], ["#f6edd0", "#fcf5d8", "#fbf8e3"], ["#ebdbb2"; 3], ["#3c3836"; 3]),
    ] {
        let file = find(id, &[]).unwrap();
        assert_eq!(file.options.len(), 1);
        let option = &file.options[0];
        assert_eq!(option.id, "contrast");
        let OptionKind::Choice { default, values } = &option.kind else { panic!("contrast must be a choice") };
        assert_eq!(default, "standard");
        assert_eq!(values.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["soft", "standard", "hard"]);
        for (index, contrast) in ["soft", "standard", "hard"].into_iter().enumerate() {
            let options = BTreeMap::from([("contrast".into(), OptionValue::Choice(contrast.into()))]);
            let tokens = resolve_with(file, &options);
            assert_eq!(tokens.panel, parse_color(panels[index]).unwrap(), "{id}: {contrast}");
            assert_eq!(tokens.viewport_top, parse_color(tops[index]).unwrap(), "{id}: {contrast}");
            assert_eq!(tokens.viewport_bottom, parse_color(bottoms[index]).unwrap(), "{id}: {contrast}");
            assert_eq!(export_icon_palette(resolve_icon_palette(file, &options))["ink"], inks[index], "{id}: {contrast}");
            assert_eq!(tokens.borders, solidcraft_theme::Borders::LEGACY);
            assert_eq!(resolve_with(&from_value(&export_file(file)).unwrap(), &options), tokens);
            assert_eq!(resolve_icon_palette(&from_value(&export_file(file)).unwrap(), &options), resolve_icon_palette(file, &options));
        }
        assert_eq!(resolve(file), resolve_base(file), "the Standard default keeps the old family palette");
    }
}

#[test]
fn gruvbox_retired_ids_stay_reserved_for_imported_names() {
    crate::test_support::host_data();
    let mut custom = Vec::new();
    for (old, _, _) in GRUVBOX_ALIASES {
        let file = from_value(&json!({"format":FORMAT,"name":old,"base":"dark"})).unwrap();
        assert_eq!(add(&mut custom, file).unwrap(), format!("custom-{old}"));
        assert!(find(old, &custom).is_none(), "retired ids are not duplicate theme cards");
    }
}

#[test]
fn onyx_defaults_offer_broad_subtle_blur_and_independent_material_choices() {
    crate::test_support::host_data();
    let file = find("onyx", &[]).unwrap();
    assert_eq!(file.base, Base::Dark);
    assert!(!file.needs_newer());
    assert_eq!(file.options.len(), 8, "seven controls plus the Fade child");
    assert_eq!(builtin().last().map(|theme| theme.id.as_str()), Some("onyx"), "existing picker order is retained");
    let (values, notices) = effective_values(file, &BTreeMap::new());
    assert!(notices.is_empty());
    assert_eq!(values["opacity"], OptionValue::Num(80.0));
    assert_eq!(values["blur"], OptionValue::Num(60.0));
    assert_eq!(values["tab_colours"], OptionValue::Bool(false));
    assert_eq!(values["tab_fade_ms"], OptionValue::Num(240.0));
    assert_eq!(values["contrast"], OptionValue::Choice("soft".into()));
    assert_eq!(values["brightness"], OptionValue::Num(100.0));
    assert!(file.options.iter().all(|option| option.id != "edge_finish"));
    assert_eq!(export_file(file)["requires"], 2);
    assert_eq!(values["accent"], OptionValue::Choice("slate".into()));
    assert_eq!(values["foreground_colours"], OptionValue::Choice("vibrant".into()));
    assert_eq!(
        ordered_options(file).iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["opacity", "blur", "tab_colours", "tab_fade_ms", "accent", "contrast", "brightness", "foreground_colours"]
    );
    let opacity = file.options.iter().find(|o| o.id == "opacity").unwrap();
    let OptionKind::Range { min, max, .. } = &opacity.kind else { panic!("opacity must be a range") };
    assert_eq!((*min, *max), (60.0, 100.0));
    let faded = resolve(file);
    let full = resolve_with(file, &BTreeMap::from([("opacity".into(), OptionValue::Num(100.0))]));
    assert!(faded.panel.a() < full.panel.a(), "the default exposes a little of the scene");
    assert_eq!((faded.toolbar, faded.timeline), (faded.panel, faded.panel), "workspace surfaces share the same material");
    assert_eq!(full.panel.a(), 255);
    assert_eq!(faded.text, full.text);
    assert!(faded.borders.primary_inset.is_some());
    assert!(faded.borders.bevel.is_none());
    assert_eq!(faded.radius_md, 6);
    assert_eq!(faded.viewport_top, resolve_base(file).viewport_top, "tab colours start off");
    for def in file.options.iter().filter(|def| !crate::theme_options::standard(&def.id)) {
        for language in ["en", "de", "ja", "ko", "zh"] {
            assert!(def.label.contains_key(language) && def.help.contains_key(language));
            if let OptionKind::Choice { values, .. } = &def.kind {
                assert!(values.iter().all(|value| value.label.contains_key(language)));
            }
        }
    }
    let roundtrip = from_value(&export_file(file)).unwrap();
    assert_eq!(roundtrip.options, file.options);
    assert_eq!(roundtrip.overlays, file.overlays);
    assert_eq!(roundtrip.tabs, file.tabs);
    assert_eq!(resolve(&roundtrip), faded);
}

#[test]
fn onyx_options_compose_and_keep_small_text_readable() {
    crate::test_support::host_data();
    use solidcraft_theme::{composite, contrast_ratio};
    let file = find("onyx", &[]).unwrap();
    let ordinary = resolve(file);
    for contrast in ["soft", "defined"] {
        for accent in ["slate", "silver", "sage"] {
            for opacity in [60.0, 80.0, 100.0] {
                for tabs in [false, true] {
                    let values = BTreeMap::from([
                        ("contrast".into(), OptionValue::Choice(contrast.into())),
                        ("accent".into(), OptionValue::Choice(accent.into())),
                        ("opacity".into(), OptionValue::Num(opacity)),
                        ("tab_colours".into(), OptionValue::Bool(tabs)),
                    ]);
                    let base = resolve_with(file, &values);
                    assert_eq!((base.toolbar, base.timeline), (base.panel, base.panel));
                    let expected = match accent {
                        "silver" => "#b7bdc2",
                        "sage" => "#aabcae",
                        _ => "#9eafbd",
                    };
                    assert_eq!(base.accent, parse_color(expected).unwrap());
                    assert!(base.borders.primary_inset.is_some());
                    assert!(base.borders.bevel.is_none());
                    assert_eq!(base.error, ordinary.error, "accent and contrast must not recolour status");
                    assert_eq!(base.warning, ordinary.warning);
                    assert_eq!(base.agent_busy, ordinary.agent_busy);
                    assert_eq!(base.agent_ok, ordinary.agent_ok);
                    assert_eq!((base.axis_x, base.axis_y, base.axis_z), (ordinary.axis_x, ordinary.axis_y, ordinary.axis_z));
                    assert_eq!(
                        export_icon_palette(resolve_icon_palette(file, &values))["solid"][2],
                        "#4da7ff",
                        "foreground glyphs retain the default Vibrant palette"
                    );
                    if contrast == "defined" {
                        assert_ne!(base.field, ordinary.field, "defined contrast separates recessed fields");
                    }
                    for tab in ["MODEL", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER", "SKETCH"] {
                        let tokens = resolve_for_tab(file, &values, Some(tab));
                        assert_eq!(tokens.accent, base.accent, "tab surfaces must not replace the selected accent");
                        assert_eq!(tokens.borders, base.borders);
                        if tabs {
                            assert_ne!(tokens.viewport_top, base.viewport_top, "{tab} has a quiet authored tint");
                        } else {
                            assert_eq!(tokens, base);
                        }
                        let workspace = crate::workspace::tab_accent(&tokens, tab);
                        assert!(contrast_ratio(workspace.ink, workspace.soft) >= 4.5);
                        assert!(contrast_ratio(workspace.on_fill(&tokens), workspace.color) >= 4.5);
                        for fill in [
                            tokens.panel,
                            tokens.panel_header,
                            tokens.toolbar,
                            tokens.timeline,
                            tokens.dialog_bg,
                            tokens.dialog_body,
                            tokens.dialog_footer,
                            tokens.overlay,
                        ] {
                            for background in [Color32::BLACK, Color32::WHITE, tokens.viewport_top, tokens.viewport_bottom] {
                                let ground = composite(fill, background);
                                for ink in [tokens.text, tokens.text_dim, tokens.form_ink, tokens.form_dim, tokens.dialog_label] {
                                    assert!(
                                        contrast_ratio(composite(ink, ground), ground) >= 4.5,
                                        "{contrast}/{accent}/{opacity}/{tabs}/{tab}: {ink:?} on {fill:?} over {background:?}"
                                    );
                                }
                            }
                        }
                        assert!(contrast_ratio(tokens.text_dim, tokens.field) >= 4.5);
                        assert!(contrast_ratio(tokens.text, tokens.control_fill) >= 4.5);
                    }
                }
            }
        }
    }
}

#[test]
fn surface_brightness_is_explicit_continuous_and_precedes_opacity_protection() {
    crate::test_support::host_data();
    use solidcraft_theme::{composite, contrast_ratio};
    let file = find("onyx", &[]).unwrap();
    let source = export_file(file);
    let index = file.options.iter().position(|option| option.id == "brightness").unwrap();
    for (key, value) in [
        ("effect", json!("unknown")),
        ("effect", json!(true)),
        ("type", json!("bool")),
        ("min", json!(59)),
        ("max", json!(121)),
        ("default", json!(90)),
        ("step", json!(2)),
        ("unit", json!("px")),
    ] {
        let mut invalid = source.clone();
        invalid["options"][index][key] = value;
        assert!(from_value(&invalid).is_err(), "invalid brightness {key}");
    }
    let mut legacy = source.clone();
    legacy["requires"] = json!(1);
    assert!(from_value(&legacy).unwrap_err().contains("requires"));
    assert_eq!(export_file(&from_value(&source).unwrap()), source);
    let mut plain = file.clone();
    plain.options.iter_mut().for_each(|option| option.effect = None);
    assert_eq!(resolve(file), resolve(&plain), "100% preserves the exact authored palette");
    let values = |brightness, opacity| {
        BTreeMap::from([
            ("brightness".into(), OptionValue::Num(brightness)),
            ("opacity".into(), OptionValue::Num(opacity)),
            ("tab_colours".into(), OptionValue::Bool(true)),
        ])
    };
    let dark = resolve_for_tab(file, &values(60.0, 100.0), Some("SHEET METAL"));
    let middle = resolve_for_tab(file, &values(100.0, 100.0), Some("SHEET METAL"));
    let bright = resolve_for_tab(file, &values(120.0, 100.0), Some("SHEET METAL"));
    assert!(dark.viewport_top.r() < middle.viewport_top.r() && middle.viewport_top.r() < bright.viewport_top.r());
    assert!(dark.panel.r() < middle.panel.r() && middle.panel.r() < bright.panel.r());
    assert_eq!((dark.panel.a(), bright.panel.a()), (middle.panel.a(), middle.panel.a()));
    assert_eq!((dark.text, dark.axis_x, dark.selected_bg), (bright.text, bright.axis_x, bright.selected_bg));
    for brightness in [60.0, 120.0] {
        let translucent = resolve_for_tab(file, &values(brightness, 60.0), Some("SHEET METAL"));
        for background in [Color32::BLACK, Color32::WHITE] {
            assert!(contrast_ratio(translucent.text, composite(translucent.panel, background)) >= 4.5);
        }
    }
}

#[test]
fn surface_brightness_preserves_alpha_and_semantics_and_scales_authored_layers() {
    crate::test_support::host_data();
    let mut source = brightness_fixture();
    source["colors"]["grid"] = json!("#64646480");
    source["colors"]["border"] = json!("#64646480");
    source["borders"] = json!({"primary":{"width":1,"color":"#64646480"}});
    let theme = from_value(&source).unwrap();
    let original = resolve(&theme);
    for percent in [60.0, 120.0] {
        let mut before = original;
        let mut after = resolve_with(&theme, &BTreeMap::from([("exposure".into(), OptionValue::Num(percent))]));
        assert_ne!(after.grid, before.grid);
        assert_ne!(after.border, before.border);
        assert_ne!(after.borders.primary, before.borders.primary);
        assert_eq!(after.borders.primary.unwrap().color.a(), before.borders.primary.unwrap().color.a());
        assert_eq!(after.borders.primary.unwrap().width, before.borders.primary.unwrap().width);
        for key in keys() {
            let alphas = |colors: &[Color32]| colors.iter().map(|color| color.a()).collect::<Vec<_>>();
            assert_eq!(alphas(slot(&mut after, key).unwrap()), alphas(slot(&mut before, key).unwrap()), "{percent} {key}: alpha");
        }
        for key in [
            "text",
            "text_dim",
            "form_ink",
            "form_dim",
            "dialog_label",
            "accent",
            "selected_bg",
            "selected_ink",
            "hover",
            "error",
            "warning",
            "agent_busy",
            "agent_ok",
            "axis_x",
            "axis_y",
            "axis_z",
            "sketch",
            "sketch_fixed",
            "sketch_conflict",
        ] {
            assert_eq!(slot(&mut after, key), slot(&mut before, key), "{percent} {key}: semantic colour");
        }
    }
    source["overlays"] = json!([{"when":{"opt":"exposure","gte":60},"colors":{"panel":"#323232","viewport_top":"#969696"}}]);
    source["tabs"] = json!({"SKETCH":{"panel":"#3c3c3c","viewport_top":"#c8c8c8"}});
    source["options"]
        .as_array_mut()
        .unwrap()
        .extend([json!({"id":"tab_colours","type":"bool","default":true}), json!({"id":"opacity","type":"range","default":60})]);
    let theme = from_value(&source).unwrap();
    for (percent, overlay_channel, tab_channel) in [(60.0, 90, 120), (120.0, 180, 240)] {
        let values = BTreeMap::from([("exposure".into(), OptionValue::Num(percent))]);
        let overlay = resolve_with(&theme, &values);
        let tab = resolve_for_tab(&theme, &values, Some("SKETCH"));
        assert_eq!(overlay.viewport_top, Color32::from_gray(overlay_channel));
        assert_eq!(tab.viewport_top, Color32::from_gray(tab_channel));
        assert_eq!(tab.axis_x, original.axis_x);
        for tokens in [overlay, tab] {
            for background in [Color32::BLACK, Color32::WHITE] {
                let ground = solidcraft_theme::composite(tokens.panel, background);
                assert!(solidcraft_theme::contrast_ratio(solidcraft_theme::composite(tokens.text, ground), ground) >= 4.5);
            }
        }
    }
}

#[test]
fn builtin_order_preserves_the_existing_picker_sequence() {
    crate::test_support::host_data();
    let themes = read_builtin(solidcraft_look::themes::BUILTIN).unwrap();
    let retained: Vec<_> = themes.iter().map(|t| t.id.as_str()).filter(|id| ORIGINAL_BUILTIN_IDS.contains(id)).collect();
    assert_eq!(retained, ORIGINAL_BUILTIN_IDS);
    assert!(solidcraft_look::themes::BUILTIN.windows(2).all(|pair| pair[0].0 < pair[1].0), "source entries are sorted by id");
}

#[test]
fn built_in_themes_read_and_differ_from_their_base() {
    crate::test_support::host_data();
    assert_eq!(builtin().len(), solidcraft_look::themes::BUILTIN.len(), "every built-in theme file reads");
    for t in builtin() {
        let base = if t.base.is_dark() { Tokens::dark() } else { Tokens::light() };
        assert_ne!(resolve(t), base, "{}", t.id);
        if t.id.starts_with("gruvbox-") || is_two_colour(&t.id) {
            assert_eq!(t.base.is_dark(), t.id.contains("dark"), "{}", t.id);
        }
    }
}

/// CGA uses only the 16 colours of the IBM Color Graphics Adapter (alpha only on translucent
/// tokens) and sets every colour, so nothing falls back to Dark's palette.
#[test]
fn cga_uses_only_the_sixteen_cga_colours() {
    crate::test_support::host_data();
    const CGA: [&str; 16] = [
        "000000", "0000aa", "00aa00", "00aaaa", "aa0000", "aa00aa", "aa5500", "aaaaaa", "555555", "5555ff", "55ff55", "55ffff", "ff5555", "ff55ff",
        "ffff55", "ffffff",
    ];
    fn colours<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::String(s) if s.starts_with('#') => out.push(s),
            Value::Array(a) => a.iter().for_each(|v| colours(v, out)),
            Value::Object(o) => o.values().for_each(|v| colours(v, out)),
            _ => {}
        }
    }
    let text = solidcraft_look::themes::BUILTIN.iter().find(|(id, _)| *id == "cga").map(|(_, text)| *text).unwrap();
    let file: Value = serde_json::from_str(text).unwrap();
    let mut found = Vec::new();
    colours(&file, &mut found);
    assert!(found.len() > keys().len(), "CGA sets every colour and its icon palette");
    for c in found {
        let rgb = c.get(1..7).unwrap_or_default().to_ascii_lowercase();
        assert!(CGA.contains(&rgb.as_str()) && parse_color(c).is_some(), "{c} is not a CGA colour");
    }
    let theme = find("cga", &[]).unwrap();
    assert_eq!((theme.name.as_str(), theme.colors.len()), ("CGA", keys().len()), "CGA sets every colour");
}

/// #4868: a theme that sets none of the File view's colours gets them
/// from its own palette, readable: the rail and primary action from its accent, the page from
/// its panel, not Dark's or Light's blue and grey. One it does set stays as set.
#[test]
fn the_file_view_follows_a_theme_that_does_not_set_its_colours() {
    crate::test_support::host_data();
    for theme in builtin() {
        let t = resolve(theme);
        // File colours derive from the authored material before floating-surface opacity.
        let mut opaque = theme.clone();
        opaque.options.retain(|option| option.id != "opacity");
        let material = resolve(&opaque);
        if !theme.colors.contains_key("file_rail") {
            assert_eq!((t.file_background, t.file_primary), (material.panel, material.accent_soft), "{}", theme.id);
            let base = if theme.base.is_dark() { Tokens::dark() } else { Tokens::light() };
            assert!(t.accent == base.accent || t.file_rail != base.file_rail, "{}: the rail is the base's", theme.id);
        }
        if !theme.colors.contains_key("file_surface") {
            assert_eq!(t.file_surface, material.dialog_bg, "{}", theme.id);
        }
        // 4.5:1, or the best any of the theme's own inks reaches on that fill when that is less
        // (Solarized Light's text on its soft accent is 4.29:1): a derived colour can't do
        // better than the theme itself.
        let own = [t.text, t.form_ink, t.accent, t.on_accent];
        let floor = |fill: Color32| own.iter().map(|i| solidcraft_theme::contrast_ratio(*i, fill)).fold(0.0, f64::max).min(4.5) - 1e-6;
        for (ink, fill) in [
            (t.file_rail_ink, t.file_rail),
            (t.file_rail_ink, t.file_rail_hover),
            (t.file_primary_ink, t.file_primary),
            (t.file_primary_ink, t.file_background),
            (t.file_dim, t.file_surface),
            (t.file_dim, t.file_background),
        ] {
            let ratio = solidcraft_theme::contrast_ratio(ink, fill);
            assert!(ratio >= floor(fill), "{}: {ink:?} on {fill:?} is {ratio:.2}:1 (floor {:.2})", theme.id, floor(fill));
        }
    }
    let green = builtin().iter().find(|t| t.id == "green-phosphor").unwrap();
    assert_ne!(resolve(green).file_rail, Tokens::dark().file_rail, "Green Phosphor's rail isn't Dark's blue");
    let own =
        parse(r##"{"format":"solidcraft-theme/1","name":"Own rail","base":"dark","colors":{"accent":"#ff0000","file_rail":"#102030"}}"##).unwrap();
    assert_eq!(resolve(&own).file_rail, Color32::from_rgb(0x10, 0x20, 0x30));
}

/// Okibi and Akari spend their ember on one or two marks a screen: the start page's rail is
/// one of their neutrals, not their accent.
#[test]
fn okibi_and_akari_rails_are_neutral() {
    crate::test_support::host_data();
    for id in ["okibi", "akari"] {
        let t = resolve(builtin().iter().find(|t| t.id == id).unwrap());
        for c in [t.file_rail, t.file_rail_hover] {
            let [r, g, b, _] = c.to_array();
            assert!(r.max(g).max(b) - r.min(g).min(b) <= 12, "{id}: {c:?} is tinted");
        }
    }
}

/// Conflicting sketch geometry (#7244) is the error red by default, follows a theme's own
/// `error` when the theme leaves `sketch_conflict` out, and takes the theme's own key when set.
#[test]
fn sketch_conflict_defaults_to_the_error_colour_and_a_theme_can_set_its_own() {
    crate::test_support::host_data();
    for t in [Tokens::light(), Tokens::dark()] {
        assert_eq!(t.sketch_conflict, t.error);
    }
    let file = |colors: &str| {
        resolve(&parse(&format!(r#"{{"format":"solidcraft-theme/1","name":"Conflict","base":"light","colors":{{{colors}}}}}"#)).unwrap())
    };
    let (red, green) = (Color32::from_rgb(0xcc, 0x24, 0x1d), Color32::from_rgb(0x12, 0xab, 0x34));
    assert_eq!(file(r##""error":"#cc241d""##).sketch_conflict, red, "the theme's error colour");
    assert_eq!(file(r##""error":"#cc241d","sketch_conflict":"#12ab34""##).sketch_conflict, green, "the theme's own key wins");
    assert_eq!(file(r##""panel":"#102030""##).sketch_conflict, Tokens::light().error);
    for theme in builtin() {
        let t = resolve(theme);
        assert_eq!(t.sketch_conflict, t.error, "{}: conflicts in the theme's error colour", theme.id);
    }
    assert!(keys().contains(&"sketch_conflict"), "exported and imported by name");
}

#[test]
fn imported_ids_never_take_a_built_in_or_mode_id_and_replace_their_own() {
    crate::test_support::host_data();
    let mut custom = Vec::new();
    let mk = |name: &str| ThemeFile { id: slug(name), name: name.into(), ..Default::default() };
    assert_eq!(add(&mut custom, mk("Gruvbox Dark")).unwrap(), "custom-gruvbox-dark");
    assert_eq!(add(&mut custom, mk("Dark")).unwrap(), "custom-dark");
    assert_eq!(add(&mut custom, mk("Mine")).unwrap(), "mine");
    let again = ThemeFile { base: Base::Light, ..mk("Mine") };
    assert_eq!(add(&mut custom, again).unwrap(), "mine");
    assert_eq!(custom.len(), 3);
    assert_eq!(custom.iter().find(|t| t.id == "mine").map(|t| t.base), Some(Base::Light));
    for i in custom.len()..MAX_CUSTOM {
        add(&mut custom, mk(&format!("t{i}"))).unwrap();
    }
    assert!(add(&mut custom, mk("one too many")).is_err());
    assert!(find("gruvbox-dark", &custom).is_some() && find("mine", &custom).is_some() && find("dark", &custom).is_none());
}

#[test]
fn built_in_shapes_match_their_character_and_light_dark_families_agree() {
    crate::test_support::host_data();
    let values = |id: &str| radius_values(resolve(find(id, &[]).unwrap()));
    for id in ["platinum-9", "green-phosphor", "amber-phosphor"] {
        assert_eq!(values(id), [0; 6], "{id}");
    }
    assert_eq!(values("solidcraft-95"), [3, 3, 4, 4, 3, 3]);
    assert_eq!(values("blueprint"), [4, 6, 8, 10, 6, 7]);
    assert_eq!(values("cupertino"), [6, 8, 12, 16, 8, 9]);
    for id in ["clay", "clay-dark"] {
        assert_eq!(values(id), [6, 9, 13, 18, 9, 10], "{id}");
    }
    let defaults = radius_values(Tokens::dark());
    assert_eq!(defaults, radius_values(Tokens::light()));
    for id in ["gruvbox-dark", "gruvbox-light"] {
        for contrast in ["soft", "standard", "hard"] {
            let options = BTreeMap::from([("contrast".into(), OptionValue::Choice(contrast.into()))]);
            assert_eq!(radius_values(resolve_with(find(id, &[]).unwrap(), &options)), defaults);
        }
    }
    for file in builtin().iter().filter(|file| file.radii.is_empty()) {
        assert_eq!(radius_values(resolve(file)), defaults, "{}", file.id);
    }
}

fn one_bit_is_complete_opaque_black_and_white(file: &ThemeFile) {
    let t = resolve(file);
    let exported = export(&file.name, file.base, &t, resolve_icon_palette(file, &BTreeMap::new()));
    let mut resolved = t;
    for key in keys() {
        for color in slot(&mut resolved, key).unwrap() {
            let [r, g, b, _] = color.to_srgba_unmultiplied();
            assert!(matches!([r, g, b], [0, 0, 0] | [255, 255, 255]), "{}: {key}: {color:?}", file.id);
        }
        assert!(file.colors.contains_key(key), "{}: missing {key}", file.id);
    }
    fn check(value: &Value) {
        match value {
            Value::String(color) if color.starts_with('#') => {
                assert!(matches!(color.as_str(), "#000000" | "#ffffff"), "{color}");
            }
            Value::Array(values) => values.iter().for_each(check),
            Value::Object(values) => values.values().for_each(check),
            _ => {}
        }
    }
    check(&exported);
    for role in solidcraft_look::icons::ROLE_NAMES.iter().filter(|role| **role != "gap") {
        assert!(file.icon_palette.contains_key(*role), "{}: missing icon role {role}", file.id);
    }
    for ramp in std::iter::once("solid").chain(solidcraft_look::icons::ACCENT_NAMES.iter().copied()) {
        assert!(file.icon_palette.contains_key(ramp), "{}: missing icon ramp {ramp}", file.id);
    }
    assert_eq!(radius_values(t), [0; 6]);
}

/// `paper` is the theme's page (panels, viewport, bodies, hover); `ink` its text, and its selection.
fn one_bit_has_full_contrast(t: &Tokens, id: &str, paper: Color32, ink: Color32) {
    assert_eq!((t.text, t.panel), (ink, paper), "{id}");
    for (fg, bg) in [
        (t.text, t.panel),
        (t.text_dim, t.field),
        (t.icon, t.toolbar),
        (t.app_bar_text, t.app_bar_tab_active),
        (t.selected_ink, t.selected_bg),
        (t.text, t.hover),
        (t.app_bar_text, t.app_bar_hover),
        (t.body_edge, t.body),
        (t.sel_edge, t.body_selected),
        (t.dimension, t.viewport_top),
        (t.sketch, t.viewport_bottom),
        (t.glyph_text, t.glyph_bg),
        (t.dialog_label, t.dialog_body),
    ] {
        assert_eq!(solidcraft_theme::contrast_ratio(fg, bg), 21.0, "{id}: {fg:?} on {bg:?}");
    }
    assert_eq!((t.viewport_top, t.viewport_bottom, t.body), (paper, paper, paper), "{id}");
    assert_eq!((t.selected_bg, t.body_selected), (ink, ink), "{id}");
    // Hover fills are the paper, so a hovered row, card or tab never swallows its ink label.
    for hover in [t.hover, t.app_bar_hover, t.app_bar_hover_subtle, t.app_bar_hover_button, t.app_bar_hover_strong] {
        assert_eq!(hover, paper, "{id}: hover fill");
    }
    assert_eq!((t.body_edge, t.selected_ink), (ink, paper), "{id}");
    // Thumbnails and other pictures keep their own pixels: the tint stays neutral white.
    assert_eq!(t.file_image_tint, Color32::WHITE, "{id}");
}

const ORIGINAL_BUILTIN_IDS: &[&str] = &[
    "gruvbox-dark",
    "gruvbox-light",
    "solidcraft-95",
    "1-bit",
    "1-bit-dark",
    "platinum-9",
    "cupertino",
    "green-phosphor",
    "amber-phosphor",
    "blueprint",
    "one-dark",
    "tokyo-night",
    "solarized-light",
    "nord",
    "high-contrast",
    "graphite",
    "nosferatu",
    "catppuccin-mocha",
    "okibi",
    "akari",
    "clay",
    "clay-dark",
    "cga",
];
