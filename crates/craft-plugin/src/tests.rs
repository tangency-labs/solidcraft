use super::*;
use serde_json::json;

fn manifest() -> Value {
    json!({"id":"sample","name":" Sample ","version":" 1 ","capabilities":["commands@0.1"],"runtime":{
        "module":"plugin.wasm","entries":["make"],"commands":["app.make"]
    }})
}
fn parse(value: &Value) -> Result<Manifest> {
    parse_manifest(&serde_json::to_vec(value).unwrap(), "sample")
}

/// Synthetic pixels only: package swatches keep their colours and never affect const inputs.
#[test]
fn swatches_validate_package_paths_sizes_types_and_images_without_refusing_the_manifest() {
    let mut value = manifest();
    let paths = [
        json!("oak.png"),
        json!("walnut.jpg"),
        json!("../outside.png"),
        json!("/outside.png"),
        json!("https://example.com/a.png"),
        json!("large.png"),
        json!("wide.png"),
        json!("wrong.gif"),
        json!("bad.png"),
        json!(7),
    ];
    let options: Vec<_> = paths.iter().enumerate().map(|(i, path)| json!({"const":i.to_string(),"x-solidcraft-swatch":path})).collect();
    value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"finish":{"type":"string","oneOf":options}}}}]);
    let parsed = parse(&value).expect("bad artwork never refuses a plugin");
    let entries = &parsed.runtime.unwrap().entries;
    let encoded = |width, height, format| {
        let pixels = image::RgbImage::from_pixel(width, height, image::Rgb([137, 61, 23]));
        let mut out = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut out, format).unwrap();
        out.into_inner()
    };
    let png = encoded(128, 128, image::ImageFormat::Png);
    let jpg = encoded(256, 256, image::ImageFormat::Jpeg);
    let wide = encoded(257, 1, image::ImageFormat::Png);
    let mut oversized_png = png.clone();
    oversized_png.resize(65537, 0);
    let report = swatches::check(entries, |path| {
        Ok(match path {
            "oak.png" => png.clone(),
            "walnut.jpg" => jpg.clone(),
            "large.png" => oversized_png.clone(),
            "wide.png" => wide.clone(),
            _ => b"bad image".to_vec(),
        })
    });
    assert_eq!(report.images.len(), 2);
    assert_eq!(report.images["oak.png"].size, [128, 128]);
    assert_eq!(&report.images["oak.png"].rgba[..4], &[137, 61, 23, 255]);
    assert_eq!(report.warnings.len(), 8, "{:?}", report.warnings);
    assert!(swatches::decode("wrong.png", b"GIF89a").is_err());
    assert!(swatches::decode("wrong.jpg", &png).is_err());
    assert!(swatches::decode("tall.png", &encoded(1, 257, image::ImageFormat::Png)).is_err());
    assert!(swatches::decode("walnut.jpeg", &jpg).is_ok());
    let mut exact_limit = png.clone();
    exact_limit.resize(65536, 0);
    assert!(swatches::decode("oak.png", &exact_limit).is_ok(), "64 KiB inclusive");
    exact_limit.push(0);
    assert!(swatches::decode("oak.png", &exact_limit).is_err(), "a valid image one byte over the limit is omitted");
    assert!(check_input(&entries[0].input_schema, &json!({"finish":"0"})).is_ok());
}

#[test]
fn swatch_files_stay_inside_the_resolved_package() {
    let root = std::env::temp_dir().join(format!("solidcraft-swatch-paths-{}", std::process::id()));
    std::fs::create_dir_all(root.join("package")).unwrap();
    std::fs::write(root.join("outside.png"), b"outside").unwrap();
    let dir = root.join("package");
    assert!(swatches::read_file(&dir, "../outside.png").is_err());
    std::fs::write(dir.join("large.png"), vec![0; 65537]).unwrap();
    assert!(swatches::read_file(&dir, "large.png").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("outside.png"), dir.join("linked.png")).unwrap();
        std::os::unix::fs::symlink(&root, dir.join("linked-dir")).unwrap();
        assert!(swatches::read_file(&dir, "linked.png").is_err());
        assert!(swatches::read_file(&dir, "linked-dir/outside.png").is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn conditional_selection_items_keep_shared_pick_objects() {
    let schema = json!({"type":"object","properties":{
        "has_pick":{"type":"boolean"},
        "faces":{"type":"array","items":{"type":"object"},"x-solidcraft-selection":{"version":1,"accept":["face"]},
            "x-solidcraft-visible-when":{"field":"has_pick","eq":true}}
    }});
    let values = json!({"has_pick":true,"faces":[{"type":"face","body":"Box","index":0,"point":[0,0,0]}]});
    assert_eq!(prepare_input(&schema, &values).unwrap(), values);
    assert!(
        strict_input_schema(&schema)["properties"]["faces"]["items"].get("additionalProperties").is_none(),
        "the MCP schema accepts the same shared pick objects"
    );
}

#[test]
fn selection_alternatives_preserve_strict_schema_parity() {
    for alternative in ["oneOf", "anyOf"] {
        for (key, annotated, selection) in [("targets", false, true), ("faces", true, true), ("records", false, false)] {
            let mut property = json!({alternative:[{"type":"array","items":{"type":"object"}}]});
            if annotated {
                property["x-solidcraft-selection"] = json!({"version":1,"accept":["face"]});
            }
            let schema = json!({"type":"object","properties":{key:property}});
            let value = json!({key:[{"type":"face","body":"Box","index":0,"point":[0,0,0]}]});
            assert_eq!(check_input(&schema, &value).is_ok(), selection, "{key}/{alternative}");
            assert_eq!(check_input_field(key, &schema["properties"][key], &value[key]).is_ok(), selection, "{key}/{alternative}");
            if selection {
                assert_eq!(prepare_input(&schema, &value).unwrap(), value);
            }
            let strict = strict_input_schema(&schema);
            assert_eq!(check_input(&strict, &value).is_ok(), selection, "{key}/{alternative}: {strict}");
            assert_eq!(strict["properties"][key][alternative][0]["items"].get("additionalProperties").is_none(), selection);
        }
    }
}

#[test]
fn filtered_selection_options_keep_the_enclosing_annotation() {
    let schema = json!({"type":"object","properties":{
        "enabled":{"type":"boolean"},
        "picks":{"x-solidcraft-selection":{"version":1,"accept":["body"]},"oneOf":[{
            "type":"array","items":{"type":"object"},"x-solidcraft-when":{"field":"enabled","eq":true}
        }]}
    }});
    assert!(check_input(&schema, &json!({"enabled":true,"picks":[{"type":"body","name":"Box"}]})).is_ok());
    assert!(matches!(
        check_input(&schema, &json!({"enabled":false,"picks":[{"type":"body","name":"Box"}]})),
        Err(HostError::InvalidInput { field, .. }) if field == "params.picks"
    ));
}

#[test]
fn conditional_composition_preserves_selection_context_across_scopes() {
    for alternative in ["oneOf", "anyOf"] {
        for (key, annotated, selection) in [("targets", false, true), ("faces", true, true), ("records", false, false)] {
            let mut picks = json!({alternative:[{"type":"array","items":{"type":"object"}}]});
            if annotated {
                picks["x-solidcraft-selection"] = json!({"version":1,"accept":["face"]});
            }
            let mut schema = json!({"type":"object","additionalProperties":true,"properties":{
                "gate":{"type":"boolean"},
                "mode":{"type":"boolean","x-solidcraft-visible-when":{"field":"gate","eq":true}},
                key:picks
            }});
            schema[alternative] = json!([{"additionalProperties":true,"required":["secret"],"properties":{
                "mode":{"type":"boolean"},
                "secret":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}}
            }}]);
            let values = json!({"gate":false,"mode":true,"secret":"hidden",key:[{"type":"face","body":"Box","index":0,"point":[0,0,0]}]});
            assert_eq!(check_input(&schema, &values).is_ok(), selection, "{key}/{alternative}");
            assert_eq!(check_input(&strict_input_schema(&schema), &values).is_ok(), selection);
            if selection {
                assert_eq!(prepare_input(&schema, &values), Ok(json!({"gate":false,key:values[key]})));
                assert!(crate::input::check_value(&schema, &values).is_err(), "Ignore still checks hidden values against their types");
                let mut visible = values.clone();
                visible["gate"] = json!(true);
                visible["secret"] = json!(7);
                assert_eq!(prepare_input(&schema, &visible), Ok(visible.clone()));
                assert_eq!(crate::input::check_value(&schema, &visible), Ok(()));
            }
        }
    }
}

#[test]
fn forms_capability_is_additive_and_versioned() {
    let mut value = manifest();
    value["capabilities"] = json!(["commands@0.1", "forms@0.1"]);
    assert!(parse(&value).is_ok());
    value["capabilities"] = json!(["commands@0.1", "forms@0.2"]);
    assert!(matches!(parse(&value), Err(HostError::UnsupportedCapability { .. })));
    assert!(parse(&manifest()).is_ok(), "existing plugins still link without forms");
}

#[test]
fn metadata_normalization_and_legacy_entries() {
    let parsed = parse(&manifest()).unwrap();
    assert_eq!(parsed.name, "Sample");
    assert_eq!(parsed.version, "1");
    let runtime = parsed.runtime.unwrap();
    assert_eq!(runtime.entries[0].name, "make");
    assert_eq!(runtime.entries[0].effect, Effect::Edit);
    let serialized = serde_json::to_value(&runtime.entries[0]).unwrap();
    assert_eq!(serialized["input_schema"]["type"], "object");
    assert_eq!(serde_json::from_value::<Entry>(serialized).unwrap().name, "make");
    assert!(parse_manifest(br#"{"id":"sample","name":"Sample","version":"1"}"#, "sample").unwrap().runtime.is_none());
}

#[test]
fn entry_images_share_bounded_paths_and_sources() {
    let schema = json!({"type":"object", "x-solidcraft-icon":"entry.svg", "properties":{
        "series":{"oneOf":[{"const":"a","x-solidcraft-icon":"icons/a.svg"},{"const":"b","x-solidcraft-icon":7}]}
    }});
    assert_eq!(images::referenced(&schema), ["entry.svg", "icons/a.svg"]);
    assert_eq!(images::annotations(&schema).len(), 3, "lint also sees non-string annotations");
    assert!(images::annotations(&json!({"type":"object"})).is_empty());
    for path in ["entry.svg", "icons/a-1.svg"] {
        assert!(images::valid_path(path));
    }
    for path in ["/entry.svg", "../entry.svg", "icons/../entry.svg", "icons\\entry.svg", "entry.png", "icons//entry.svg"] {
        assert!(!images::valid_path(path), "{path}");
    }
    let mut svg = b"<svg viewBox=\"0 0 64 64\"></svg>".to_vec();
    assert!(images::source(&svg).is_ok());
    svg.resize(images::MAX_BYTES, b' ');
    assert!(images::source(&svg).is_ok(), "the size limit is inclusive");
    svg.push(b' ');
    assert!(images::source(&svg).is_err());
    assert!(images::source(b"<svg viewBox='0 0 32 32'></svg>").is_err());
    assert!(images::source(&[0xff]).is_err());
}

#[test]
fn declared_inputs_effects_and_slots_are_readable() {
    let mut value = manifest();
    value["runtime"]["entries"] = json!([{"name":"inspect","effect":"read",
        "input_schema":{"type":"object","properties":{"selection":{"type":"array","items":{"type":"string"}}}},
        "slots":[{"kind":"menu","name":"tools"},{"kind":"ribbon","name":"utilities"}]}]);
    let entry = parse(&value).unwrap().runtime.unwrap().entries.remove(0);
    assert_eq!(entry.effect, Effect::Read);
    assert_eq!(entry.slots.len(), 2);
    assert_eq!(entry.input_schema["properties"]["selection"]["type"], "array");
    value["runtime"]["entries"][0]["input_schema"]["properties"]["selection"]["type"] = json!("string");
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field.ends_with("selection")));
}

#[test]
fn ribbon_groups_parse_in_declared_order_and_are_opt_in() {
    let runtime = parse(&manifest()).unwrap().runtime.unwrap();
    assert!(runtime.ribbon_groups.is_empty());
    assert!(serde_json::to_value(runtime).unwrap().get("ribbon_groups").is_none(), "old manifests keep their serialized shape");
    let mut value = manifest();
    value["runtime"]["entries"] = json!(["make", "inspect", "other"]);
    value["runtime"]["ribbon_groups"] = json!([
        {"label":"Gears","icon":"icons/gears.svg","entries":["inspect","make"]},
        {"label":"Other","entries":["other"]}
    ]);
    let runtime = parse(&value).unwrap().runtime.unwrap();
    assert_eq!(runtime.ribbon_groups[0].label, "Gears");
    assert_eq!(runtime.ribbon_groups[0].icon.as_deref(), Some("icons/gears.svg"));
    assert_eq!(runtime.ribbon_groups[0].entries, ["inspect", "make"]);
    assert_eq!(runtime.ribbon_groups[1].icon, None);
    let serialized = serde_json::to_value(&runtime).unwrap();
    assert_eq!(serialized["ribbon_groups"], value["runtime"]["ribbon_groups"]);
    value["runtime"] = serialized;
    assert_eq!(parse(&value).unwrap().runtime.unwrap().ribbon_groups, runtime.ribbon_groups);
}

#[test]
fn ribbon_groups_refuse_unknown_entries_empty_groups_and_duplicates() {
    for (groups, field, message) in [
        (json!([{"label":"Gears","entries":["missing"]}]), "0.entries", "unknown entry `missing`"),
        (json!([{"label":"Gears","entries":[]}]), "0.entries", "1..32 distinct entry names"),
        (json!([{"label":"Gears","entries":["make","make"]}]), "0.entries", "entry `make` is listed twice"),
        (json!([{"label":"Gears","entries":["make"]},{"label":"More","entries":["make"]}]), "1.entries", "entry `make` is listed twice"),
        (json!([{"label":"Gears","entries":["make"]},{"label":" Gears ","entries":["inspect"]}]), "1.label", "ribbon group `Gears` is listed twice"),
    ] {
        let mut value = manifest();
        value["runtime"]["entries"] = json!(["make", "inspect"]);
        value["runtime"]["ribbon_groups"] = groups;
        let error = parse(&value).unwrap_err();
        assert!(
            matches!(&error, HostError::InvalidManifest { field: f, reason } if f == &format!("runtime.ribbon_groups.{field}") && reason.contains(message)),
            "{error}"
        );
    }
}

#[test]
fn ribbon_groups_refuse_hostile_labels_icons_and_malformed_fields() {
    for group in [
        json!({"label":"","entries":["make"]}),
        json!({"label":"   ","entries":["make"]}),
        json!({"label":"Gears\n","entries":["make"]}),
        json!({"label":"x".repeat(81),"entries":["make"]}),
        json!({"label":"Gears","icon":"../gears.svg","entries":["make"]}),
        json!({"label":"Gears","icon":"/gears.svg","entries":["make"]}),
        json!({"label":"Gears","icon":"https://example.org/gears.svg","entries":["make"]}),
        json!({"label":"Gears","icon":"gears.png","entries":["make"]}),
        json!({"label":"Gears","icon":"","entries":["make"]}),
        json!({"label":"Gears","icon":42,"entries":["make"]}),
        json!({"label":"Gears","entries":"make"}),
        json!({"label":"Gears","entries":["make"],"unknown":true}),
        json!({"label":"Gears"}),
    ] {
        let mut value = manifest();
        value["runtime"]["ribbon_groups"] = json!([group]);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })), "{value}");
    }
    let mut value = manifest();
    value["runtime"]["ribbon_groups"] = json!(vec![json!({"label":"Gears","entries":["make"]}); 33]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "runtime.ribbon_groups"));
}

#[test]
fn declared_property_order_survives_manifest_and_entry_round_trips() {
    let bytes = br#"{"id":"sample","name":"Sample","version":"1","capabilities":["commands@0.1"],"runtime":{
        "module":"plugin.wasm","commands":[],"entries":[{"name":"make","input_schema":{
            "type":"object","x-solidcraft-field-order":"declared","required":["alpha","zeta"],"properties":{
                "zeta":{"type":"number","default":1.5},
                "extra":{"type":"number","default":2,"x-solidcraft-visible-when":{"field":"mode","eq":true}},
                "mode":{"type":"boolean","default":false},
                "alpha":{"type":"object","properties":{
                    "z/n~":{"type":"array","items":{"type":"object","properties":{
                        "tail":{"type":"string","default":"tail"},
                        "head":{"type":["string","null"],"default":null}
                    }}},
                    "a":{"type":"boolean","default":true}
                }}
            },"allOf":[{"properties":{"after":{"type":"number"},"before":{"type":"number"}}}]
        }}]
    }}"#;
    let manifest = parse_manifest(bytes, "sample").unwrap();
    let mut entry = manifest.runtime.as_ref().unwrap().entries[0].clone();
    let original: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(entry.input_schema, original["runtime"]["entries"][0]["input_schema"], "schema values retain ordinary JSON behaviour");
    for _ in 0..2 {
        assert_eq!(entry.input_property_order[""], ["zeta", "extra", "mode", "alpha"]);
        assert_eq!(entry.input_property_order["/properties/alpha"], ["z/n~", "a"]);
        assert_eq!(entry.input_property_order["/properties/alpha/properties/z~1n~0/items"], ["tail", "head"]);
        assert_eq!(entry.input_property_order["/allOf/0"], ["after", "before"]);
        entry = serde_json::from_value(serde_json::to_value(&entry).unwrap()).unwrap();
    }
    let round_trip = parse_manifest(&serde_json::to_vec(&manifest).unwrap(), "sample").unwrap();
    assert_eq!(round_trip.runtime.unwrap().entries[0].input_property_order, entry.input_property_order);
}

#[test]
fn dialog_hook_requires_its_capability_and_newer_versions_are_reported_first() {
    let mut m = manifest();
    m["runtime"]["entries"] = json!([{"name":"make","dialog_hook":true}]);
    assert!(matches!(parse(&m), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    m["capabilities"] = json!(["commands@0.1", "dialog@0.1"]);
    assert!(parse(&m).unwrap().runtime.unwrap().entries[0].dialog_hook);
    m["capabilities"] = json!(["commands@0.1", "dialog@0.2"]);
    m["runtime"]["entries"][0]["future_hook_options"] = json!({"new":true});
    assert!(matches!(parse(&m), Err(HostError::UnsupportedCapability { capability, .. }) if capability == "dialog@0.2"));
}

fn hook_schema() -> Value {
    json!({"type":"object","required":["width"],"properties":{
        "mode":{"type":"boolean","default":false},
        "width":{"type":"number","minimum":1},
        "locked":{"type":"number","x-solidcraft-visible-when":{"field":"mode","eq":false},"x-solidcraft-enabled-when":{"field":"mode","eq":false}},
        "size":{"type":"string","oneOf":[{"const":"small","x-solidcraft-when":{"field":"mode","eq":false}},{"const":"large"}]}
    }})
}

#[test]
fn dialog_hook_applies_unconditional_schema_defaults_without_overwriting_inputs() {
    let schema = json!({"type":"object","required":["width"],"properties":{
        "width":{"type":"number","default":3,"minimum":1}
    }});
    assert!(conditions::InputConditions::parse(&schema).unwrap().order.is_empty());
    for reply in [None, Some("{}"), Some("not JSON")] {
        let update = dialog::apply(&schema, &json!({}), &BTreeSet::new(), reply, true).unwrap();
        assert_eq!(update.values, json!({"width":3}));
        assert_eq!(update.warning.is_some(), reply == Some("not JSON"));
        let update = dialog::apply(&schema, &json!({"width":12}), &BTreeSet::from(["width".into()]), reply, true).unwrap();
        assert_eq!(update.values, json!({"width":12}));
    }
    let update = dialog::apply(&schema, &json!({"width":7}), &BTreeSet::new(), Some("{}"), true).unwrap();
    assert_eq!(update.values, json!({"width":7}));
    // An edited, cleared field is absent from a partial form's values but remains protected.
    let update = dialog::apply(&schema, &json!({}), &BTreeSet::from(["width".into()]), Some("{}"), false).unwrap();
    assert_eq!(update.values, json!({}));
}

#[test]
fn dialog_hook_defaults_and_narrowed_choices_share_input_validation() {
    let patch = r#"{"width":{"default":24},"size":{"choices":["large"],"hint":"Available locally"}}"#;
    let update = dialog::apply(&hook_schema(), &json!({}), &BTreeSet::new(), Some(patch), true).unwrap();
    assert_eq!(update.values["width"], 24);
    assert!(update.warning.is_none());
    let input = json!({"width":12,"size":"small"});
    let partial = dialog::apply(&hook_schema(), &input, &BTreeSet::from(["width".into(), "size".into()]), Some(patch), false).unwrap();
    assert_eq!(partial.values["size"], "small", "forms preserve an edited choice and gate Run until it is valid");
    assert_eq!(partial.fields["size"]["choices"], json!(["large"]));
    let error = dialog::apply(&hook_schema(), &input, &BTreeSet::from(["width".into(), "size".into()]), Some(patch), true).unwrap_err();
    assert!(matches!(error, HostError::InvalidInput { field, reason } if field == "params.size" && reason.contains("size")));
    let update = dialog::apply(&hook_schema(), &json!({"width":12}), &BTreeSet::from(["width".into()]), Some(patch), true).unwrap();
    assert_eq!(update.values["width"], 12);
}

#[test]
fn dialog_hook_computed_controllers_recheck_declarative_defaults() {
    let schema = json!({"type":"object","properties":{
        "mode":{"type":"boolean","default":false},
        "width":{"type":"number","default":3,"x-solidcraft-defaults":[{"when":{"field":"mode","eq":true},"value":6}]}
    }});
    let reply = r#"{"mode":{"value":true}}"#;
    let update = dialog::apply(&schema, &json!({}), &BTreeSet::new(), Some(reply), true).unwrap();
    assert_eq!(update.values, json!({"mode":true,"width":6}));
    let update = dialog::apply(&schema, &json!({"width":12}), &BTreeSet::from(["width".into()]), Some(reply), true).unwrap();
    assert_eq!(update.values["width"], 12);
}

#[test]
fn dialog_hook_bad_reply_is_ignored_whole_and_cannot_widen_choices_or_conditions() {
    for reply in ["not JSON", "[]", "null"] {
        let update = dialog::apply(&hook_schema(), &json!({}), &BTreeSet::new(), Some(reply), false).unwrap();
        assert!(update.warning.is_some());
        assert!(update.fields.is_empty());
        assert!(update.values.get("width").is_none());
    }
    for patch in [
        json!({"width":{"default":24},"missing":{"hint":"unknown"}}),
        json!({"width":{"default":24},"size":{"choices":["other"]}}),
        json!({"width":{"default":24},"size":{"choices":["large","small"]}}),
        json!({"width":{"default":24},"size":{"choices":["small"]}}),
        json!({"width":{"type":"string"}}),
        json!({"width":{"default":"wrong"}}),
        json!({"width":{"default":null}}),
        json!({"width":{"default":24},"locked":{"visible":true}}),
        json!({"width":{"default":24},"locked":{"enabled":true}}),
    ] {
        let update = dialog::apply(&hook_schema(), &json!({"mode":true}), &BTreeSet::new(), Some(&patch.to_string()), false).unwrap();
        assert!(update.warning.is_some(), "{patch}");
        assert!(update.fields.is_empty(), "no partial patch");
        assert!(update.values.get("width").is_none(), "no partial default");
    }
}

#[test]
fn design_listeners_require_a_read_entry_and_design_capability() {
    let mut value = manifest();
    value["capabilities"] = json!(["commands@0.1", "design@0.1"]);
    value["runtime"]["entries"] = json!([{"name":"refresh","effect":"read"}]);
    value["runtime"]["listeners"] = json!([{"entry":"refresh","on":["design.changed"]}]);
    let runtime = parse(&value).unwrap().runtime.unwrap();
    assert_eq!(runtime.listeners[0].entry, "refresh");
    assert_eq!(runtime.listeners[0].on, ["design.changed"]);
    for events in [json!(["design.links_changed"]), json!(["design.changed", "design.links_changed"])] {
        let mut opted_in = value.clone();
        opted_in["runtime"]["listeners"][0]["on"] = events.clone();
        let runtime = parse(&opted_in).unwrap().runtime.unwrap();
        assert_eq!(serde_json::to_value(&runtime.listeners[0].on).unwrap(), events);
    }
    for change in [
        |v: &mut Value| v["capabilities"] = json!(["commands@0.1"]),
        |v: &mut Value| v["runtime"]["entries"][0]["effect"] = json!("edit"),
        |v: &mut Value| v["runtime"]["listeners"][0]["entry"] = json!("missing"),
        |v: &mut Value| v["runtime"]["listeners"][0]["on"] = json!(["unknown"]),
        |v: &mut Value| v["runtime"]["listeners"][0]["on"] = json!(["design.links_changed", "unknown"]),
        |v: &mut Value| v["runtime"]["listeners"][0]["on"] = json!(["design.changed", "design.changed"]),
        |v: &mut Value| v["runtime"]["listeners"][0]["on"] = json!(["design.links_changed", "design.links_changed"]),
        |v: &mut Value| v["runtime"]["listeners"][0]["on"] = json!([]),
        |v: &mut Value| {
            v["runtime"]["listeners"] = json!([
            {"entry":"refresh","on":["design.changed"]}, {"entry":"refresh","on":["design.changed"]}])
        },
    ] {
        let mut invalid = value.clone();
        change(&mut invalid);
        assert!(matches!(parse(&invalid), Err(HostError::InvalidManifest { .. })));
    }
    assert!(parse(&manifest()).unwrap().runtime.unwrap().listeners.is_empty());
}

#[test]
fn optional_design_capabilities_authorize_listeners_only_when_supported() {
    let mut value = manifest();
    value["capabilities"] = json!(["commands@0.2", "host@0.1"]);
    value["runtime"]["entries"] = json!([{"name":"refresh","effect":"read"}]);
    value["runtime"]["listeners"] = json!([{"entry":"refresh","on":["design.changed"]}]);
    for capability in ["design@0.1", "design@0.1.3"] {
        value["optional_capabilities"] = json!([capability, "future@0.1"]);
        let parsed = parse(&value).unwrap();
        assert_eq!(parsed.granted_capabilities(), ["commands@0.2", "host@0.1", capability]);
        assert_eq!(parsed.runtime.unwrap().listeners[0].entry, "refresh");
    }
    value["optional_capabilities"] = json!(["design@99.0"]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "runtime.listeners"));
    value["runtime"]["listeners"] = json!([]);
    assert_eq!(parse(&value).unwrap().granted_capabilities(), ["commands@0.2", "host@0.1"]);
}

#[test]
fn stale_revision_is_a_typed_error_naming_both_tokens() {
    let expected = DesignToken { document_id: "design-one".into(), revision: "0123456789abcdef0123456789abcdef:3".into() };
    let actual = DesignToken { document_id: "design-one".into(), revision: "0123456789abcdef0123456789abcdef:4".into() };
    let error = HostError::StaleRevision { expected: expected.clone(), actual: actual.clone() };
    assert_eq!(serde_json::to_value(&error).unwrap(), json!({"kind":"stale_revision","expected":expected,"actual":actual}));
    assert!(error.to_string().contains("design-one@0123456789abcdef0123456789abcdef:3"));
    assert!(error.to_string().contains("design-one@0123456789abcdef0123456789abcdef:4"));
}

#[test]
fn design_tokens_bound_hostile_document_identities() {
    for identity in [String::new(), "x".repeat(129), "line\nbreak".into(), "tab\t".into(), "nul\0".into(), "delete\u{7f}".into(), "café".into()] {
        let wire = json!({"document_id":identity,"revision":"0123456789abcdef0123456789abcdef:7"});
        assert!(serde_json::from_value::<DesignToken>(wire).is_err(), "{identity:?}");
    }
    for identity in ["design-one".to_string(), "x".repeat(128), " !~".into()] {
        let wire = json!({"document_id":identity,"revision":"0123456789abcdef0123456789abcdef:7"});
        let token: DesignToken = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(token).unwrap(), wire);
    }
    let schema = schemars::schema_for!(DesignToken).to_value();
    assert_eq!(schema["properties"]["document_id"]["minLength"], 1);
    assert_eq!(schema["properties"]["document_id"]["maxLength"], 128);
}

#[test]
fn design_tokens_bound_hostile_revisions() {
    for revision in [String::new(), "x".repeat(129), "line\nbreak".into(), "tab\t".into(), "nul\0".into(), "delete\u{7f}".into(), "café".into()] {
        let wire = json!({"document_id":"design-one","revision":revision});
        assert!(serde_json::from_value::<DesignToken>(wire).is_err(), "{revision:?}");
    }
    assert!(serde_json::from_value::<DesignToken>(json!({"document_id":"design-one","revision":7})).is_err());
    for revision in ["0123456789abcdef0123456789abcdef:7".to_string(), "x".repeat(128), " !~".into()] {
        let wire = json!({"document_id":"design-one","revision":revision});
        let token: DesignToken = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(token).unwrap(), wire);
    }
    let schema = schemars::schema_for!(DesignToken).to_value();
    assert_eq!(schema["properties"]["revision"]["type"], "string");
    assert_eq!(schema["properties"]["revision"]["minLength"], 1);
    assert_eq!(schema["properties"]["revision"]["maxLength"], 128);
}

#[test]
fn schema_properties_must_be_objects_even_in_nested_inputs() {
    for schema in [
        json!({"type":"object","properties":[]}),
        json!({"type":"object","properties":null}),
        json!({"type":"object","properties":{"settings":{"type":"object","properties":false}}}),
    ] {
        let mut value = manifest();
        value["runtime"]["entries"] = json!([{"name":"make","input_schema":schema}]);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field.ends_with(".properties")));
    }
}

#[test]
fn field_order_annotation_only_accepts_declared() {
    let mut value = manifest();
    value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object"}}]);
    assert!(parse(&value).is_ok(), "the annotation is optional");
    value["runtime"]["entries"][0]["input_schema"]["x-solidcraft-field-order"] = json!("declared");
    assert!(parse(&value).is_ok(), "declared order is an explicit opt-in");
    for order in [json!("alphabetical"), json!("Declared"), json!(""), json!(true), json!(3), Value::Null, json!(["declared"]), json!({})] {
        value["runtime"]["entries"][0]["input_schema"]["x-solidcraft-field-order"] = order.clone();
        assert!(
            matches!(parse(&value), Err(HostError::InvalidManifest { field, reason })
                if field == "runtime.entries.make.input_schema.x-solidcraft-field-order" && reason.contains("\"declared\"")),
            "invalid field-order annotation: {order}"
        );
    }
}

#[test]
fn network_permission_is_a_typed_refusal() {
    let mut value = manifest();
    value["runtime"]["permissions"] = json!({"net":true});
    assert_eq!(parse(&value).unwrap_err(), HostError::PermissionDenied { permission: "net".into() });
}

#[test]
fn named_files_permissions_require_access_and_unique_names() {
    let mut value = manifest();
    value["runtime"]["permissions"] = json!({"files":[{"name":"inputs","read":true}],"clock":true});
    let permissions = parse(&value).unwrap().runtime.unwrap().permissions;
    assert!(permissions.clock);
    assert!(permissions.files[0].read);
    value["runtime"]["permissions"]["files"] = json!([{"name":"inputs"}]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
    value["runtime"]["permissions"]["files"] = json!([{"name":"inputs","read":true},{"name":"inputs","write":true}]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
}

#[test]
fn hostile_manifest_fields_return_errors() {
    assert!(matches!(
        parse_manifest(&serde_json::to_vec(&manifest()).unwrap(), "another-folder"),
        Err(HostError::InvalidManifest { field, reason }) if field == "id" && reason.contains("does not match") && reason.contains("rename the folder to")
    ));
    for change in [
        |value: &mut Value| value["runtime"]["module"] = json!("../plugin.wasm"),
        |value: &mut Value| value["runtime"]["entries"] = json!(["make", "make"]),
        |value: &mut Value| value["runtime"]["limits"] = json!({"fuel":0,"memory_mib":64,"deadline_ms":1}),
        |value: &mut Value| value["runtime"]["limits"] = json!({"fuel":1,"memory_mib":4097,"deadline_ms":1}),
        |value: &mut Value| value["runtime"]["limits"] = json!({"fuel":1,"memory_mib":1,"deadline_ms":30001}),
        |value: &mut Value| value["runtime"]["entries"] = json!([{"name":"make","input_schema":true}]),
    ] {
        let mut value = manifest();
        change(&mut value);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
    }
    assert!(matches!(parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1], "sample"), Err(HostError::LimitExceeded { .. })));
    assert!(parse_manifest(br#"{}"#, "sample").is_err());
}

/// A refused entry list says which entry is wrong and why: a bad name (an underscore, which
/// command ids allow but entry names don't), a repeated name, or too many or no entries.
#[test]
fn a_refused_entry_list_names_the_entry_and_the_rule() {
    let refusal = |entries: Value| {
        let mut value = manifest();
        value["runtime"]["entries"] = entries;
        match parse(&value) {
            Err(HostError::InvalidManifest { field, reason }) => (field, reason),
            other => panic!("expected a refusal, got {other:?}"),
        }
    };
    let (field, reason) = refusal(json!(["make", "op_add"]));
    assert_eq!(field, "runtime.entries");
    assert!(reason.contains("\"op_add\"") && reason.contains("dashes") && reason.contains("no underscores"), "{reason}");
    let (_, reason) = refusal(json!(["make", "make"]));
    assert!(reason.contains("\"make\" is declared more than once"), "{reason}");
    let (_, reason) = refusal(json!([]));
    assert!(reason.contains("declares 0 entries"), "{reason}");
    let many: Vec<String> = (0..33).map(|i| format!("e{i}")).collect();
    let (_, reason) = refusal(json!(many));
    assert!(reason.contains("declares 33 entries"), "{reason}");
    // A hostile name is quoted and cut short, never echoed raw.
    let (_, reason) = refusal(json!([format!("bad\nname{}", "x".repeat(500))]));
    assert!(reason.contains("\\n") && !reason.contains('\n') && reason.len() < 400, "{reason}");
    // A cut name says it was cut and how long it is: its first 64 characters may be valid alone.
    let (_, reason) = refusal(json!([format!("{}_", "a".repeat(64))]));
    assert!(reason.contains(&format!("\"{}\"… (65 characters)", "a".repeat(64))), "{reason}");
}

#[test]
fn larger_manifests_fit_without_removing_input_descriptions() {
    let serialized = serde_json::to_vec(&manifest()).unwrap();
    assert!(serialized.len() < 16 * 1024);
    assert_eq!(MAX_MANIFEST_BYTES, 64 * 1024);

    // Whitespace pads a real manifest to exact byte boundaries without changing its
    // meaning. The old 16 KiB cap no longer rejects a valid manifest.
    for len in [16 * 1024 + 1, 64 * 1024] {
        let mut bytes = serialized.clone();
        bytes.resize(len, b' ');
        let parsed = parse_manifest(&bytes, "sample").unwrap();
        assert_eq!(parsed.id, "sample");
    }

    let mut too_large = serialized;
    too_large.resize(MAX_MANIFEST_BYTES + 1, b' ');
    assert!(matches!(parse_manifest(&too_large, "sample"), Err(HostError::LimitExceeded { limit }) if limit.contains("64 KiB")));
}

#[test]
fn entries_share_field_definitions_through_defs_and_ref() {
    let place = json!({"type":"string","title":"Place","default":"auto","enum":["auto","origin"]});
    let units = json!({"type":"string","enum":["mm","in"]});
    let entries = |make: Value, edit: Value| json!([{"name":"make","input_schema":make},{"name":"edit","input_schema":edit}]);
    let mut inline = manifest();
    inline["runtime"]["entries"] = entries(
        json!({"type":"object","properties":{"place":place,"units":units}}),
        json!({"type":"object","properties":{"place":{"type":"string","title":"Place","default":"origin","enum":["auto","origin"]}}}),
    );
    // One definition in the manifest, one in an entry, and a `default` beside a `$ref` overriding.
    let mut shared = manifest();
    shared["$defs"] = json!({"place":place});
    shared["runtime"]["entries"] = entries(
        json!({"type":"object","$defs":{"units":units},"properties":{"place":{"$ref":"#/$defs/place"},"units":{"$ref":"#/$defs/units"}}}),
        json!({"type":"object","properties":{"place":{"$ref":"#/$defs/place","default":"origin"}}}),
    );
    let expanded = parse(&shared).unwrap().runtime.unwrap().entries;
    let expected = parse(&inline).unwrap().runtime.unwrap().entries;
    for (expanded, expected) in expanded.iter().zip(&expected) {
        assert_eq!(expanded.input_schema, expected.input_schema, "every consumer sees the inline schema");
        assert_eq!(strict_input_schema(&expanded.input_schema), strict_input_schema(&expected.input_schema));
    }
    assert!(check_input(&expanded[0].input_schema, &json!({"place":"origin","units":"mm"})).is_ok());
    assert!(check_input(&expanded[0].input_schema, &json!({"place":"x","units":"mm"})).is_err(), "a shared field keeps its choices");

    let refused = |defs: Value, property: Value| {
        let mut value = manifest();
        value["$defs"] = defs;
        value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"a":property}}}]);
        matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "runtime.entries.make.input_schema")
    };
    let cycle = json!({"a":{"type":"object","properties":{"b":{"$ref":"#/$defs/b"}}},"b":{"type":"object","properties":{"a":{"$ref":"#/$defs/a"}}}});
    assert!(refused(cycle, json!({"$ref":"#/$defs/a"})), "a cycle is refused");
    assert!(refused(json!({}), json!({"$ref":"#/$defs/missing"})), "an unknown name is refused");
    assert!(refused(json!({"a":{"type":"string"}}), json!({"$ref":"other.json#/a"})), "only local definitions");
    // Refs that double at every level would multiply the manifest; they stop at a limit.
    let mut doubling = serde_json::Map::new();
    for level in 0..20 {
        let next = json!({"$ref": format!("#/$defs/d{}", level + 1)});
        doubling.insert(format!("d{level}"), json!({"type":"object","properties":{"x":next,"y":next}}));
    }
    doubling.insert("d20".into(), json!({"type":"string"}));
    assert!(refused(Value::Object(doubling), json!({"$ref":"#/$defs/d0"})));
}

#[test]
fn host_limits_have_typed_failures() {
    let limits = HostLimits::default();
    assert!(limits.check(&Limits::default()).is_ok());
    let fuel = limits.check(&Limits { fuel: limits.max_fuel + 1, ..Limits::default() }).unwrap_err();
    assert_eq!(fuel, HostError::OverCeiling { limit: "fuel".into(), requested: limits.max_fuel + 1, allowed: limits.max_fuel });
    assert!(fuel.to_string().contains("allow its fuel request when you install it"));
    // Approved per-package fuel reaches HostLimits::check just as approved memory does.
    let approved = HostLimits { max_fuel: MAX_APPROVED_FUEL, ..limits };
    assert!(approved.check(&Limits { fuel: MAX_APPROVED_FUEL, ..Limits::default() }).is_ok());
    let hard = HostError::OverCeiling { limit: "fuel".into(), requested: MAX_APPROVED_FUEL + 1, allowed: MAX_APPROVED_FUEL };
    assert!(hard.to_string().contains("hard host ceiling of 10000000000000"));
    // A refusal gives both numbers and what to change.
    let memory = limits.check(&Limits { memory_mib: 2048, ..Limits::default() }).unwrap_err();
    assert_eq!(memory, HostError::OverCeiling { limit: "memory_mib".into(), requested: 2048, allowed: 128 });
    assert_eq!(
        memory.to_string(),
        "asks for 2048 MiB of memory, more than the 128 MiB plugins may use; allow its memory request when you install it, or raise the memory ceiling for plugins"
    );
    assert_eq!(serde_json::to_value(&memory).unwrap(), json!({"kind":"over_ceiling","limit":"memory_mib","requested":2048,"allowed":128}));
    assert!(HostLimits { max_fuel: 0, ..limits }.validate().is_err());
    // A budget the plugin ran into names it, with the number.
    let stopped = |limit: &str, allowed| HostError::Exhausted { limit: limit.into(), allowed }.to_string();
    assert_eq!(stopped("fuel", 1000), "used all 1000 fuel it asks for and was stopped");
    assert_eq!(stopped("memory_mib", 64), "needed more than the 64 MiB of memory it asks for and was stopped");
    assert_eq!(stopped("deadline_ms", 5000), "ran longer than its time limit of 5000 ms and was stopped");
    assert_eq!(stopped("design_mib", 256), "made a design larger than 256 MiB, the most an importer may hand over");
    assert_eq!(HostError::Unsupported { reason: "no solids".into() }.to_string(), "not supported: no solids");
}

#[test]
fn host_errors_are_machine_readable() {
    let error = HostError::CommandFailed { command: "app.make".into(), reason: "missing part".into() };
    let value = serde_json::to_value(&error).unwrap();
    assert_eq!(value["kind"], "command_failed");
    assert_eq!(value["command"], "app.make");
    assert_eq!(serde_json::from_value::<HostError>(value).unwrap(), error);
    assert!(WIT.contains(&format!("package {API};")));
}

#[test]
fn explicit_folder_grants_allow_hidden_ancestors_but_refuse_hidden_children() {
    struct Host(Vec<FileGrant>);
    impl PluginHost for Host {
        fn command_catalog(&self) -> Vec<CommandSchema> {
            vec![]
        }
        fn run_command(&mut self, _: &str, _: &Value) -> Result<Value> {
            Ok(Value::Null)
        }
        fn begin_transaction(&mut self) -> Result<()> {
            Ok(())
        }
        fn commit_transaction(&mut self) -> Result<()> {
            Ok(())
        }
        fn rollback_transaction(&mut self) -> Result<()> {
            Ok(())
        }
        fn granted_folders(&self) -> Vec<FileGrant> {
            self.0.clone()
        }
        fn log(&mut self, _: &str) {}
        fn progress(&mut self, _: f32, _: &str) {}
    }
    let folder = Path::new("parent").join(".private").join("approved");
    for (read, write) in [(true, true), (true, false), (false, true)] {
        let host = Host(vec![FileGrant { name: "project".into(), folder: folder.clone(), read, write }]);
        assert_eq!(host.authorize_file_path(&folder.join("ordinary.txt"), false).is_ok(), read);
        assert_eq!(host.authorize_file_path(&folder.join("ordinary.txt"), true).is_ok(), write);
        for operation in [false, true] {
            for child in [Path::new(".env").to_owned(), Path::new(".nested").join("ordinary.txt")] {
                assert!(matches!(host.authorize_file_path(&folder.join(child), operation), Err(HostError::PermissionDenied { .. })));
            }
        }
    }
    let mut host = Host(vec![FileGrant { name: "project".into(), folder: folder.clone(), read: true, write: true }]);
    host.0.push(FileGrant { name: "explicit-hidden".into(), folder: folder.join(".nested"), read: true, write: false });
    assert!(host.authorize_file_path(&folder.join(".nested/ordinary.txt"), false).is_ok());
    assert!(host.authorize_file_path(&folder.join(".nested/ordinary.txt"), true).is_err());
    assert!(Host(vec![]).authorize_file_path(&folder.join("ordinary.txt"), false).is_err());
}

#[test]
fn ui_panels_are_a_supported_capability() {
    assert_eq!(check_capabilities(&["ui@0.1".into()], "Craft"), Ok(()));
}

fn panel_manifest() -> Value {
    let mut value = manifest();
    value["capabilities"] = json!(["commands@0.2", "ui@0.1"]);
    value["runtime"]["ui"] = json!({"ui_version":1,"panels":[{
        "id":"tools","title":"Tools","zones":["right"],"layout":{"root":0,"nodes":[
            {"widget":{"button":{"label":"Make","icon":null,"entry":"make"}}}
        ]}
    }],"workspaces":[]});
    value
}

#[test]
fn panel_layouts_are_bounded_trees_with_declared_actions() {
    let valid = panel_manifest();
    let parsed = parse(&valid).unwrap();
    let runtime = parsed.runtime.as_ref().unwrap();
    let declarations = runtime.ui.as_ref().unwrap();
    let mut event = ui::UiEvent { panel: "tools".into(), event: ui::EventKind::Press, path: "make".into(), row: None, value: None };
    assert_eq!(declarations.action_command("sample", 0, &event).unwrap(), "sample.make");
    event.path = "other".into();
    assert!(declarations.action_command("sample", 0, &event).is_err());
    assert!(declarations.action_command("sample", u32::MAX, &event).is_err());
    for malformed in [
        json!({"root":1,"nodes":[{"widget":{"text":"x"}}]}),
        json!({"root":0,"nodes":[{"widget":{"group":[0]}}]}),
        json!({"root":0,"nodes":[{"widget":{"group":[1,1]}},{"widget":{"text":"x"}}]}),
        json!({"root":0,"nodes":[{"widget":{"text":"x"}},{"widget":{"text":"unused"}}]}),
        json!({"root":0,"nodes":[{"widget":{"button":{"label":"No","entry":"undeclared"}}}]}),
    ] {
        let mut value = valid.clone();
        value["runtime"]["ui"]["panels"][0]["layout"] = malformed;
        assert!(parse(&value).is_err(), "{value}");
    }
    let mut large = valid.clone();
    large["runtime"]["ui"]["panels"][0]["layout"]["nodes"] = json!(vec![json!({"widget":{"text":"x"}}); ui::MAX_WIDGETS + 1]);
    assert!(matches!(parse(&large), Err(HostError::LimitExceeded { .. })));
    let mut too_many = valid.clone();
    let panel = too_many["runtime"]["ui"]["panels"][0].clone();
    too_many["runtime"]["ui"]["panels"] = json!(
        (0..=ui::MAX_PANELS)
            .map(|index| {
                let mut panel = panel.clone();
                panel["id"] = json!(format!("panel-{index}"));
                panel
            })
            .collect::<Vec<_>>()
    );
    assert!(matches!(parse(&too_many), Err(HostError::LimitExceeded { .. })));
    let mut bad_zones = valid.clone();
    bad_zones["runtime"]["ui"]["panels"][0]["zones"] = json!(["left", "left"]);
    assert!(parse(&bad_zones).is_err());
    let mut importing = importer();
    importing["capabilities"] = json!(["formats@0.2", "ui@0.1"]);
    importing["runtime"]["ui"] = json!({"ui_version":1,"panels":[],"workspaces":[]});
    assert!(parse(&importing).is_err());
    let mut missing_capability = valid.clone();
    missing_capability["capabilities"] = json!(["commands@0.2"]);
    assert!(parse(&missing_capability).is_err());
    let mut future = valid.clone();
    future["runtime"]["ui"]["ui_version"] = json!(2);
    future["runtime"]["ui"]["panels"][0]["layout"]["nodes"][0]["widget"] = json!({"future-widget":{"unfamiliar":"data"}});
    assert!(matches!(parse(&future).unwrap().runtime.unwrap().ui.unwrap().panels[0].layout.nodes[0].widget, ui::Widget::Unknown));
}

#[test]
fn panel_row_actions_preserve_opaque_ids_with_slashes() {
    let mut value = panel_manifest();
    value["runtime"]["ui"]["panels"][0]["layout"]["nodes"][0]["widget"] = json!({"table":{
        "bind":"/jobs", "row_id":"id", "columns":[{"key":"id","label":"Operation","unit":null}],
        "select":"single", "on_select":"make", "on_edit":"make"
    }});
    let runtime = parse(&value).unwrap().runtime.unwrap();
    let declarations = runtime.ui.as_ref().unwrap();
    let layout = &declarations.panels[0].layout;
    let mut data = ui::PanelData::default();
    data.set_data_for(layout, &[ui::Datum { path: "/jobs/0/id".into(), value: ui::UiValue::Text("setup/op1".into()) }]).unwrap();
    assert!(data.merge_frame());
    assert_eq!(data.rows("/jobs")[0].0, "0", "the data index stays separate from the opaque row id");
    assert_eq!(data.get("/jobs/0/id"), Some(&ui::UiValue::Text("setup/op1".into())));
    for event in [ui::EventKind::Select, ui::EventKind::Edit] {
        let event = ui::UiEvent { panel: "tools".into(), event, path: "/jobs".into(), row: Some("setup/op1".into()), value: None };
        assert_eq!(declarations.action_command("sample", 0, &event), Ok("sample.make".into()), "{event:?}");
        let encoded = serde_json::to_value(&event).unwrap();
        assert_eq!(encoded["path"], "/jobs");
        assert_eq!(encoded["row"], "setup/op1");
        let mut wrong_binding = event.clone();
        wrong_binding.path = "/jobs/0".into();
        assert!(declarations.action_command("sample", 0, &wrong_binding).is_err());
        for row in ["".to_owned(), "setup/\nop1".to_owned(), "x".repeat(1025)] {
            let mut hostile = event.clone();
            hostile.row = Some(row);
            assert!(declarations.action_command("sample", 0, &hostile).is_err());
        }
    }
}

#[test]
fn panel_data_updates_are_atomic_and_merged_once_per_frame() {
    let mut data = ui::PanelData::default();
    let datum = |path: &str, value| ui::Datum { path: path.into(), value };
    data.set_data(&[datum("/tools/10/dia", ui::UiValue::Number(10.0))]).unwrap();
    assert_eq!(data.get("/tools/10/dia"), None);
    data.set_data(&[datum("/tools/2/dia", ui::UiValue::Number(2.0))]).unwrap();
    assert!(data.merge_frame());
    assert!(!data.merge_frame());
    assert_eq!(data.rows("/tools").into_iter().map(|(row, _)| row).collect::<Vec<_>>(), ["2", "10"]);
    data.set_data(&[datum("/tools/15x/dia", ui::UiValue::Number(15.0))]).unwrap();
    data.merge_frame();
    assert_eq!(data.rows("/tools").into_iter().map(|(row, _)| row).collect::<Vec<_>>(), ["2", "10", "15x"]);
    let original = data.clone();
    assert!(data.set_data(&[datum("/tools/2/dia", ui::UiValue::Number(3.0)), datum("bad", ui::UiValue::Flag(true))]).is_err());
    assert!(data.set_data(&[datum("/tools/2/dia", ui::UiValue::Number(f64::NAN))]).is_err());
    assert!(data.set_data(&[datum("/oversize", ui::UiValue::Text("x".repeat(ui::MAX_PANEL_BYTES)))]).is_err());
    assert_eq!(data, original);
    assert!(!data.merge_frame());
    data.clear_data();
    assert_eq!(data.get("/tools/2/dia"), Some(&ui::UiValue::Number(2.0)));
    data.set_data(&[datum("/replacement", ui::UiValue::Text("value".into()))]).unwrap();
    assert!(data.merge_frame());
    assert_eq!(data.values().len(), 1);
    assert_eq!(data.get("/replacement"), Some(&ui::UiValue::Text("value".into())));
}

#[test]
fn panel_frame_adoption_keeps_the_same_logical_state() {
    let empty = ui::PanelData::default();
    let mut data = empty.clone();
    data.set_data(&[ui::Datum { path: "/value".into(), value: ui::UiValue::Integer(4) }]).unwrap();
    let pending = data.clone();
    assert_ne!(pending, empty);
    assert_eq!(pending.get("/value"), None);
    assert!(data.merge_frame());
    assert_eq!(data, pending, "rendering a frame must not make a worker's snapshot stale");
    assert_eq!(data.get("/value"), Some(&ui::UiValue::Integer(4)));
    data.clear_data();
    assert_eq!(data, empty);
    assert!(data.merge_frame());
    assert_eq!(data, empty);
}

#[test]
fn chart_data_point_limits_reject_whole_updates() {
    let layout = ui::Layout {
        root: 0,
        nodes: vec![ui::Node {
            widget: ui::Widget::Chart(ui::ChartSpec {
                kind: ui::ChartKind::Line,
                series: vec![ui::ChartSeries { label: "Load".into(), role: ui::ColorRole::Series1, x: "/x".into(), y: "/y".into() }],
            }),
        }],
    };
    let mut data = ui::PanelData::default();
    let updates: Vec<_> = (0..=ui::MAX_CHART_POINTS).map(|index| ui::Datum { path: format!("/x/{index}"), value: ui::UiValue::None }).collect();
    assert!(matches!(data.set_data_for(&layout,&updates),Err(HostError::LimitExceeded {limit}) if limit.contains("20000")));
    assert_eq!(data, ui::PanelData::default());
    data.set_data_for(&layout, &updates[..ui::MAX_CHART_POINTS]).unwrap();
    assert!(data.merge_frame());
}

#[test]
fn panel_wit_preserves_the_reviewed_interfaces() {
    // Keep this shared crate movable: the normative RFC is outside the extracted crate.
    for signature in [
        "interface ui {",
        "interface ui-provider {",
        "set-data: func(panel: string, updates: list<datum>) -> result<_, plugin-error>;",
        "clear-data: func(panel: string) -> result<_, plugin-error>;",
        "show-panel: func(panel: string, visible: bool) -> result<_, plugin-error>;",
        "describe: func() -> declarations;",
        "on-event: func(event: ui-event) -> result<_, plugin-error>;",
        "on-enter: func(workspace: string) -> result<_, plugin-error>;",
        "on-exit: func(workspace: string) -> result<_, plugin-error>;",
    ] {
        assert!(WIT.contains(signature), "{signature}");
    }
    assert!(WIT.contains("world panel-plugin"));
    assert!(WIT.contains("world panel-dialog-plugin"));
    assert_eq!(serde_json::to_value(ui::ColorRole::Series1).unwrap(), "series-1");
}

#[test]
fn capabilities_are_checked_at_load_with_plain_refusals() {
    assert!(WIT.starts_with("package craft:plugin@0.2.0;") && API == "craft:plugin@0.2.0");
    // The frozen 0.1 WIT is 0.1.0 without `unsupported`, and the host still serves its capabilities.
    assert!(WIT_0_1.contains("\npackage craft:plugin@0.1.0;") && !WIT_0_1.contains("unsupported") && WIT.contains("unsupported(string)"));
    assert_eq!(WIT_VERSIONS.first().copied(), API.split_once('@').map(|(_, v)| v));
    // The commands capability is the WIT package's version.
    let (_, version) = API.split_once('@').unwrap();
    let (major, minor) = CAPABILITIES.iter().find(|(name, _)| *name == "commands").unwrap().1;
    assert!(version.starts_with(&format!("{major}.{minor}.")));
    let caps = |list: &[&str]| list.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    assert!(check_capabilities(&caps(&["commands@0.1"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["commands@0.1.3"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["commands@0.2"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["formats@0.1", "evaluate@0.1"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["formats@0.2", "evaluate@0.2"]), "Craft").is_ok());
    let refused = |list: &[&str]| match check_capabilities(&caps(list), "Craft") {
        Err(HostError::UnsupportedCapability { reason, .. }) => reason,
        other => panic!("{list:?}: {other:?}"),
    };
    assert_eq!(refused(&["commands@0.3"]), "needs commands 0.3; update Craft or the plugin");
    assert_eq!(refused(&["commands@1.0"]), "needs commands 1.0; update Craft or the plugin");
    assert!(refused(&["commands@0.0"]).contains("was built for commands 0.0") && refused(&["commands@0.0"]).ends_with("update the plugin"));
    assert!(refused(&["viewport@0.1"]).starts_with("needs viewport 0.1, which this version of Craft does not have"));
    for bad in [&["commands"][..], &["commands@x"], &["commands@0.1", "commands@0.1"], &["Commands@0.1"], &["commands@0.1.2.3"]] {
        assert!(matches!(check_capabilities(&caps(bad), "Craft"), Err(HostError::InvalidManifest { .. })), "{bad:?}");
    }
    // A runtime needs the commands capability; inert metadata needs none.
    let mut m = manifest();
    m.as_object_mut().unwrap().remove("capabilities");
    assert!(matches!(parse_manifest(m.to_string().as_bytes(), "sample"), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    assert!(parse_manifest(br#"{"id":"sample","name":"Sample","version":"1"}"#, "sample").is_ok());
    // A plugin from before capabilities is told what it was built for, not a parse error.
    let mut old = manifest();
    old["runtime"]["api"] = json!("solidcraft:plugin@1.0.0");
    let refusal = parse_manifest(old.to_string().as_bytes(), "sample").unwrap_err().to_string();
    assert!(refusal.contains("was built for solidcraft:plugin@1.0.0") && refusal.contains("craft:plugin@0.2.0"), "{refusal}");
    // Sensitive command groups are named, bounded requests.
    let mut groups = manifest();
    groups["runtime"]["permissions"] = json!({"command_groups":["file","doc"]});
    assert_eq!(parse_manifest(groups.to_string().as_bytes(), "sample").unwrap().runtime.unwrap().permissions.command_groups.len(), 2);
    groups["runtime"]["permissions"] = json!({"command_groups":["File"]});
    assert!(parse_manifest(groups.to_string().as_bytes(), "sample").is_err());
}

#[test]
fn minimum_host_version_is_checked_before_fields_capabilities_and_legacy_api() {
    let mut value = manifest();
    value["min_host_version"] = json!("2.1.0");
    for change in [
        |value: &mut Value| value["future_field"] = json!(true),
        |value: &mut Value| value["capabilities"] = json!(["future@0.1"]),
        |value: &mut Value| value["runtime"]["api"] = json!("future:plugin@9.0.0"),
    ] {
        let mut future = value.clone();
        change(&mut future);
        let refusal = parse_manifest_for_version(future.to_string().as_bytes(), "sample", "SolidCraft", "2.0.9").unwrap_err();
        assert!(matches!(&refusal, HostError::InvalidManifest { field, .. } if field == "min_host_version"));
        let message = refusal.to_string();
        assert!(message.contains("SolidCraft 2.1.0") && message.contains("SolidCraft is 2.0.9"), "{message}");
        assert!(!message.contains("future_field") && !message.contains("future@") && !message.contains("future:plugin"), "{message}");
    }
    assert!(parse_manifest_for_version(value.to_string().as_bytes(), "sample", "SolidCraft", "2.1.0").is_ok());
    value["min_host_version"] = json!(env!("CARGO_PKG_VERSION"));
    assert!(parse(&value).is_ok(), "the default parser uses the crate's host version");
}

#[test]
fn minimum_host_versions_use_semantic_version_precedence() {
    for (minimum, current, supported) in [
        ("1.2.3", "1.2.2", false),
        ("1.2.3", "1.2.3", true),
        ("1.2.3", "1.10.0", true),
        ("1.2.3", "2.0.0", true),
        ("1.2.3", "1.2.3-rc.1", false),
        ("1.2.3-rc.1", "1.2.3", true),
        ("1.2.3-rc.2", "1.2.3-rc.10", true),
        ("1.2.3-rc.10", "1.2.3-rc.2", false),
        ("1.2.3+build.z", "1.2.3+build.a", true),
        ("1.2.3+build.a", "1.2.3", true),
        ("0.0.0", "0.0.0", true),
    ] {
        let mut value = manifest();
        value["min_host_version"] = json!(minimum);
        let parsed = parse_manifest_for_version(value.to_string().as_bytes(), "sample", "SolidCraft", current);
        assert_eq!(parsed.is_ok(), supported, "minimum {minimum}, current {current}: {parsed:?}");
        if supported {
            assert_eq!(parsed.unwrap().min_host_version.as_deref(), Some(minimum));
        }
    }
}

#[test]
fn minimum_host_version_requires_a_complete_semantic_version_string() {
    for minimum in [
        json!(null),
        json!(true),
        json!(123),
        json!([]),
        json!({}),
        json!(""),
        json!("1"),
        json!("1.2"),
        json!("v1.2.3"),
        json!(" 1.2.3"),
        json!("1.2.3 "),
        json!("01.2.3"),
        json!("1.2.3-01"),
        json!("1.2.3+"),
        json!("1.2.3.4"),
        json!("18446744073709551616.0.0"),
    ] {
        let mut value = manifest();
        value["min_host_version"] = minimum.clone();
        value["unknown_field"] = json!(true);
        let parsed = parse_manifest_for_version(value.to_string().as_bytes(), "sample", "SolidCraft", "2.0.0");
        assert!(matches!(parsed, Err(HostError::InvalidManifest { field, .. }) if field == "min_host_version"), "{minimum}");
    }
}

#[test]
fn extension_metadata_accepts_unknown_keys_but_other_manifest_fields_stay_strict() {
    let mut value = manifest();
    let legacy = parse(&value).unwrap();
    assert!(legacy.min_host_version.is_none() && legacy.optional_capabilities.is_empty() && legacy.extensions.is_empty());
    let serialized = serde_json::to_value(legacy).unwrap();
    for field in ["min_host_version", "optional_capabilities", "extensions"] {
        assert!(serialized.get(field).is_none(), "empty {field} is omitted");
    }
    value["extensions"] = json!({"future":{"arbitrary":[null,true,3,"text",{"another":false}]},"vendor.example":42,
        "min_host_version":"999.0.0","capabilities":["future@0.1"]});
    let parsed = parse(&value).unwrap();
    assert_eq!(json!(parsed.extensions), value["extensions"]);
    assert_eq!(parsed.granted_capabilities(), ["commands@0.1"]);
    for extension in [json!(null), json!(false), json!(42), json!("metadata"), json!([])] {
        value["extensions"] = extension;
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
    }
    value["extensions"] = json!({});
    value["future"] = json!(true);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { reason, .. }) if reason.contains("unknown field `future`")));
    value.as_object_mut().unwrap().remove("future");
    value["runtime"]["future"] = json!(true);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { reason, .. }) if reason.contains("unknown field `future`")));
}

#[test]
fn optional_capabilities_grant_supported_versions_and_preserve_declared_strings() {
    let mut value = manifest();
    value["optional_capabilities"] = json!(["host@0.1.7", "future@0.1", "forms@0.1", "dialog@0.1", "evaluate@0.3"]);
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.optional_capabilities.len(), 5);
    assert_eq!(parsed.granted_capabilities(), ["commands@0.1", "host@0.1.7", "forms@0.1", "dialog@0.1"]);
    for version in ["host@0.0", "host@0.2", "host@1.0", "host@0.1"] {
        value["optional_capabilities"] = json!([version]);
        assert_eq!(parse(&value).unwrap().granted_capabilities().len(), if version == "host@0.1" { 2 } else { 1 }, "{version}");
    }
    let mut importing = importer();
    importing["optional_capabilities"] = json!(["evaluate@0.1.4", "host@0.1", "future@0.1"]);
    assert_eq!(parse(&importing).unwrap().granted_capabilities(), ["formats@0.1", "evaluate@0.1.4", "host@0.1"]);
}

#[test]
fn optional_capabilities_are_bounded_well_formed_and_never_duplicate_required_names() {
    for optional in [
        json!(["future"]),
        json!(["future@x"]),
        json!(["Future@0.1"]),
        json!(["future@0.1.2.3"]),
        json!(["future@0.1-beta"]),
        json!(["future@4294967296.1"]),
        json!(["future@0.1", "future@0.2"]),
        json!(["commands@0.2"]),
    ] {
        let mut value = manifest();
        value["optional_capabilities"] = optional.clone();
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "optional_capabilities"), "{optional}");
    }
    for optional in [json!(null), json!({}), json!("host@0.1"), json!([true])] {
        let mut value = manifest();
        value["optional_capabilities"] = optional;
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
    }
    let mut value = manifest();
    value["optional_capabilities"] = json!((0..16).map(|index| format!("future-{index}@0.1")).collect::<Vec<_>>());
    assert_eq!(parse(&value).unwrap().granted_capabilities(), ["commands@0.1"]);
    value["optional_capabilities"].as_array_mut().unwrap().push(json!("future-16@0.1"));
    assert!(
        matches!(parse(&value), Err(HostError::InvalidManifest { field, reason }) if field == "optional_capabilities" && reason.contains("at most 16"))
    );
    value["optional_capabilities"] = json!([]);
    value["capabilities"] = json!((0..17).map(|index| format!("future-{index}@0.1")).collect::<Vec<_>>());
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, reason }) if field == "capabilities" && reason.contains("at most 16")));
}

#[test]
fn optional_capabilities_keep_runtime_kind_and_storage_requirements() {
    let mut command = manifest();
    command["capabilities"] = json!([]);
    command["optional_capabilities"] = json!(["commands@0.1"]);
    assert!(matches!(parse(&command), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    let mut importing = importer();
    importing["capabilities"] = json!([]);
    importing["optional_capabilities"] = json!(["formats@0.1"]);
    assert!(matches!(parse(&importing), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    let mut command = manifest();
    command["optional_capabilities"] = json!(["evaluate@0.1"]);
    assert!(matches!(parse(&command), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    command["optional_capabilities"] = json!(["storage@0.1"]);
    assert!(matches!(parse(&command), Err(HostError::InvalidManifest { field, .. }) if field == "runtime.data_format"));
    command["runtime"]["data_format"] = json!(1);
    assert_eq!(parse(&command).unwrap().granted_capabilities(), ["commands@0.1", "storage@0.1"]);
    command["optional_capabilities"] = json!(["storage@0.2"]);
    assert!(matches!(parse(&command), Err(HostError::InvalidManifest { field, .. }) if field == "runtime.data_format"));
    command["runtime"].as_object_mut().unwrap().remove("data_format");
    assert_eq!(parse(&command).unwrap().granted_capabilities(), ["commands@0.1"]);
    importing["capabilities"] = json!(["formats@0.1"]);
    importing["optional_capabilities"] = json!(["storage@0.1"]);
    importing["runtime"]["data_format"] = json!(1);
    assert!(matches!(parse(&importing), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
}

fn importer() -> Value {
    json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.1"],"runtime":{
        "module":"sample.wasm","commands":[],"entries":[{"name":"convert","kind":"importer","extensions":["f3d","ipt"]}],
        "limits":{"fuel":1000,"memory_mib":512,"deadline_ms":300000,"stack_kib":16384},"permissions":{"input_folder":true}
    }})
}

#[test]
fn an_importer_claims_extensions_needs_formats_and_never_mixes_with_commands() {
    let runtime = parse(&importer()).unwrap().runtime.unwrap();
    assert!(runtime.is_importer());
    assert_eq!(runtime.max_component_bytes(), MAX_IMPORTER_COMPONENT_BYTES);
    assert_eq!(runtime.importer_for(Path::new("/x/Bracket.F3D")).map(|e| e.name.as_str()), Some("convert"));
    assert!(runtime.importer_for(Path::new("bracket.step")).is_none());
    let refused = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = importer();
        change(&mut value);
        let failure = parse(&value).unwrap_err();
        assert!(matches!(&failure, HostError::InvalidManifest { field: f, .. } if f.contains(field)), "{field}: {failure}");
    };
    refused(&|v| v["capabilities"] = json!(["commands@0.1"]), "capabilities");
    refused(&|v| v["runtime"]["entries"][0]["extensions"] = json!([]), "extensions");
    refused(&|v| v["runtime"]["entries"][0]["extensions"] = json!([".f3d"]), "extensions");
    refused(&|v| v["runtime"]["entries"][0]["extensions"] = json!(["f3d", "f3d"]), "extensions");
    refused(&|v| v["runtime"]["entries"][0]["effect"] = json!("read"), "effect");
    // An importer opens a file as a new design: that is its effect, whether it says so, says
    // nothing, or says `edit` as manifests written before `open` did; a command never opens.
    assert_eq!(runtime.entries[0].effect, Effect::Open);
    assert_eq!(serde_json::to_value(&runtime.entries[0]).unwrap()["effect"], "open");
    for declared in ["open", "edit"] {
        let mut value = importer();
        value["runtime"]["entries"][0]["effect"] = json!(declared);
        assert_eq!(parse(&value).unwrap().runtime.unwrap().entries[0].effect, Effect::Open, "{declared}");
    }
    refused(&|v| v["runtime"]["entries"].as_array_mut().unwrap().push(json!({"name":"make"})), "runtime.entries");
    refused(&|v| v["runtime"]["commands"] = json!(["app.make"]), "runtime.commands");
    refused(&|v| v["runtime"]["limits"]["stack_kib"] = json!(1 << 20), "stack_kib");
    // An importer may also check its result; a command may not ask to.
    let mut evaluating = importer();
    evaluating["capabilities"] = json!(["evaluate@0.1", "formats@0.1"]);
    assert!(parse(&evaluating).is_ok());
    // A command claims no extension, reads no input folder and still finishes within 30 s.
    let command = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = manifest();
        change(&mut value);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field: f, .. }) if f.contains(field)), "{field}");
    };
    command(&|v| v["runtime"]["entries"] = json!([{"name":"make","extensions":["f3d"]}]), "extensions");
    command(&|v| v["runtime"]["entries"] = json!([{"name":"make","effect":"open"}]), "effect");
    command(&|v| v["runtime"]["permissions"] = json!({"input_folder":true}), "input_folder");
    command(&|v| v["capabilities"] = json!(["commands@0.1", "evaluate@0.1"]), "capabilities");
    command(&|v| v["runtime"]["limits"] = json!({"fuel":1,"memory_mib":1,"deadline_ms":60000}), "deadline_ms");
}

fn exporter() -> Value {
    json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.1"],"runtime":{
        "module":"sample.wasm","commands":["document.inspect"],
        "entries":[{"name":"mesh","kind":"exporter","extensions":["smp"],"label":"Sample mesh","needs_plugin_data":true,
                    "input_schema":{"type":"object","properties":{"units":{"type":"string","enum":["mm","in"]}}}}],
        "limits":{"fuel":1000,"memory_mib":512,"deadline_ms":300000}
    }})
}

#[test]
fn an_export_slot_claims_extensions_reads_the_design_and_asks_for_no_files() {
    let runtime = parse(&exporter()).unwrap().runtime.unwrap();
    assert!(runtime.is_exporter() && !runtime.is_importer());
    assert_eq!(runtime.max_component_bytes(), MAX_IMPORTER_COMPONENT_BYTES);
    let slot = &runtime.entries[0];
    assert_eq!((slot.label.as_str(), slot.effect, slot.needs_plugin_data), ("Sample mesh", Effect::Read, true));
    assert!(slot.writes(Path::new("/x/Part.SMP")) && !slot.claims(Path::new("/x/part.smp")));
    let refused = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = exporter();
        change(&mut value);
        let failure = parse(&value).unwrap_err();
        assert!(matches!(&failure, HostError::InvalidManifest { field: f, .. } if f.contains(field)), "{field}: {failure}");
    };
    refused(&|v| v["capabilities"] = json!(["commands@0.1"]), "capabilities");
    refused(&|v| v["capabilities"] = json!(["formats@0.1", "evaluate@0.1"]), "capabilities");
    refused(&|v| v["runtime"]["entries"][0]["extensions"] = json!([]), "extensions");
    refused(&|v| v["runtime"]["entries"][0]["effect"] = json!("edit"), "effect");
    refused(&|v| v["runtime"]["entries"][0]["label"] = json!("two\nlines"), "label");
    refused(
        &|v| v["runtime"]["entries"].as_array_mut().unwrap().push(json!({"name":"open","kind":"importer","extensions":["smp"]})),
        "runtime.entries",
    );
    // An exporter writes only the file the user chooses: no folders, groups or input folder.
    refused(&|v| v["runtime"]["permissions"] = json!({"files":[{"name":"out","write":true}]}), "runtime.permissions");
    refused(&|v| v["runtime"]["permissions"] = json!({"command_groups":["file"]}), "runtime.permissions");
    refused(&|v| v["runtime"]["permissions"] = json!({"input_folder":true}), "runtime.permissions");
    // Only an export slot reads plugin data; a command has no label.
    let mut importing = importer();
    importing["runtime"]["entries"][0]["needs_plugin_data"] = json!(true);
    assert!(matches!(parse(&importing), Err(HostError::InvalidManifest { field, .. }) if field.contains("needs_plugin_data")));
    let mut command = manifest();
    command["runtime"]["entries"] = json!([{"name":"make","label":"Make"}]);
    assert!(parse(&command).is_err());
    // A capability this host lacks is named first, whatever else the manifest holds.
    let mut newer = exporter();
    newer["capabilities"] = json!(["formats@0.3"]);
    newer["runtime"]["entries"][0]["kind"] = json!("printer");
    assert!(matches!(parse(&newer), Err(HostError::UnsupportedCapability { .. })));
}

#[test]
fn user_storage_is_additive_and_needs_a_command_or_importer_runtime_but_no_design_format() {
    let mut value = manifest();
    for capability in ["user-storage@0.1", "user-storage@0.1.0"] {
        value["capabilities"] = json!(["commands@0.1", capability]);
        assert!(parse(&value).is_ok());
    }
    value["capabilities"] = json!(["commands@0.2", "storage@0.1", "user-storage@0.1"]);
    value["runtime"]["data_format"] = json!(1);
    assert!(parse(&value).is_ok());
    value["capabilities"] = json!(["commands@0.2", "user-storage@0.2"]);
    assert!(matches!(parse(&value), Err(HostError::UnsupportedCapability { .. })));
    let mut value = importer();
    for capability in ["user-storage@0.1", "user-storage@0.1.0"] {
        value["capabilities"] = json!(["formats@0.2", capability]);
        assert!(parse(&value).is_ok());
    }
    value["runtime"]["data_format"] = json!(1);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "runtime.data_format"));
    value["capabilities"] = json!(["formats@0.2", "storage@0.1", "user-storage@0.1"]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    let mut value = manifest();
    value["capabilities"] = json!(["user-storage@0.1"]);
    value.as_object_mut().unwrap().remove("runtime");
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
}

#[test]
fn user_storage_budgets_include_json_overhead_and_keys_use_utf8_bytes() {
    // {"k":""} is eight bytes; allow exactly 64 KiB, refuse one more byte.
    let exact = BTreeMap::from([("k".into(), json!("x".repeat(MAX_USER_STORAGE_BYTES - 8)))]);
    assert_eq!(serde_json::to_vec(&exact).unwrap().len(), MAX_USER_STORAGE_BYTES);
    check_user_storage(&exact).unwrap();
    let larger = BTreeMap::from([("k".into(), json!("x".repeat(MAX_USER_STORAGE_BYTES - 7)))]);
    assert_eq!(check_user_storage(&larger).unwrap_err(), HostError::LimitExceeded { limit: "user-storage total bytes (65536)".into() });
    let mut keys: BTreeMap<_, _> = (0..MAX_USER_STORAGE_KEYS).map(|i| (format!("k{i}"), json!(null))).collect();
    check_user_storage(&keys).unwrap();
    keys.insert("k0".into(), json!({"replacement":true}));
    check_user_storage(&keys).unwrap();
    keys.insert("one-more".into(), json!(null));
    assert_eq!(check_user_storage(&keys).unwrap_err(), HostError::LimitExceeded { limit: "user-storage keys (128)".into() });
    check_user_storage_key(&"é".repeat(64)).unwrap();
    assert!(check_user_storage_key(&"é".repeat(65)).is_err());
    assert!(check_user_storage_key("bad\nkey").is_err());
}

#[test]
fn a_command_may_be_a_long_job_and_an_importer_may_not() {
    let mut long = manifest();
    long["runtime"]["entries"] = json!([{"name":"make","job":true}, "other"]);
    let entries = parse(&long).unwrap().runtime.unwrap().entries;
    assert_eq!((entries[0].job, entries[1].job), (true, false));
    let mut importing = importer();
    importing["runtime"]["entries"][0]["job"] = json!(true);
    assert!(matches!(parse(&importing), Err(HostError::InvalidManifest { field, .. }) if field.ends_with(".job")));
}

#[test]
fn a_plugin_that_keeps_design_data_names_storage_and_its_data_format() {
    let mut keeps = manifest();
    keeps["capabilities"] = json!(["commands@0.1", "storage@0.1"]);
    keeps["runtime"]["data_format"] = json!(3);
    assert_eq!(parse(&keeps).unwrap().runtime.unwrap().data_format, Some(3));
    let refused = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = keeps.clone();
        change(&mut value);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field: f, .. }) if f == field), "{field}");
    };
    refused(
        &|v| {
            v["runtime"].as_object_mut().unwrap().remove("data_format");
        },
        "runtime.data_format",
    );
    refused(&|v| v["runtime"]["data_format"] = json!(0), "runtime.data_format");
    refused(&|v| v["runtime"]["data_format"] = json!(MAX_DATA_FORMAT + 1), "runtime.data_format");
    refused(&|v| v["capabilities"] = json!(["commands@0.1"]), "runtime.data_format");
    // An importer keeps nothing in the open design.
    let mut importing = importer();
    importing["capabilities"] = json!(["formats@0.1", "storage@0.1"]);
    importing["runtime"]["data_format"] = json!(1);
    assert!(matches!(parse(&importing), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    // An exporter only reads the open design and keeps no data there.
    let mut exporting = exporter();
    exporting["capabilities"] = json!(["formats@0.1", "storage@0.1"]);
    exporting["runtime"]["data_format"] = json!(1);
    assert!(matches!(parse(&exporting), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
}

fn empty_item() -> LossItem {
    LossItem { severity: LossSeverity::Info, title: String::new(), subject: String::new(), message: String::new(), code: String::new() }
}

#[test]
fn a_loss_report_is_bounded_printable_text() {
    let report: LossReport = serde_json::from_value(json!({"summary":"12 of 14 features","items":[
        {"severity":"degraded","subject":"Fillet3","message":"kept as geometry"},
        {"severity":"info","title":"Parts of the file kept as they are","code":"f3d/source.unindexed-records-retained","message":"14 records"}],
        "details":{"any":"thing"}}))
    .unwrap();
    report.validate().unwrap();
    assert_eq!(report.items[1].title, "Parts of the file kept as they are");
    let long_title = json!({"items":[{"severity":"info","title":"t".repeat(121),"message":"x"}]});
    assert!(serde_json::from_value::<LossReport>(long_title).unwrap().validate().is_err());
    assert!(serde_json::from_value::<LossReport>(json!({"items":[{"severity":"lost","message":"x"}]})).is_err());
    let control = LossReport { summary: "bad\nline".into(), ..LossReport::default() };
    assert!(matches!(control.validate(), Err(HostError::InvalidInput { field, .. }) if field == "loss_report.summary"));
    let empty = LossReport {
        items: vec![LossItem { severity: LossSeverity::Info, subject: String::new(), message: " ".into(), ..empty_item() }],
        ..LossReport::default()
    };
    assert!(empty.validate().is_err());
}

#[test]
fn font_annotation_is_a_string_hint_and_does_not_restrict_input() {
    let property = json!({"type":"string","x-solidcraft-font":true});
    let mut value = manifest();
    value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"font":property}}}]);
    let entry = parse(&value).unwrap().runtime.unwrap().entries.remove(0);
    assert_eq!(strict_input_schema(&entry.input_schema)["properties"]["font"][FONT_KEY], true);
    for font in ["Liberation Sans", "Unknown family", "", "/fonts/custom.ttf"] {
        assert_eq!(check_input(&entry.input_schema, &json!({"font":font})), Ok(()));
    }
    assert!(check_input(&entry.input_schema, &json!({"font":17})).is_err());
    for property in [
        json!({"type":"number","x-solidcraft-font":true}),
        json!({"type":"string","x-solidcraft-font":"true"}),
        json!({"type":"string","x-solidcraft-font":false}),
        json!({"type":"string","x-solidcraft-font":{}}),
    ] {
        value["runtime"]["entries"][0]["input_schema"]["properties"]["font"] = property;
        assert!(parse(&value).is_err(), "malformed font hint must be rejected");
    }
}

#[test]
fn entry_arguments_are_checked_as_strictly_as_app_commands() {
    let schema = json!({"type":"object","required":["teeth"],"properties":{
        "teeth":{"type":"integer","minimum":6,"maximum":200},
        "module":{"type":"number","exclusiveMinimum":0},
        "name":{"type":"string","maxLength":8},
        "up_axis":{"type":"string","oneOf":[{"const":"as_is","title":"As in the file"},{"const":"stand_up","title":"Stand up"}]},
        "bodies":{"type":"array","items":{"type":"string"},"maxItems":2},
        "free":{"type":"object","additionalProperties":true},
        "fixed":{"type":"object"}
    }});
    assert_eq!(check_input(&schema, &json!({"teeth": 20})), Ok(()));
    assert_eq!(check_input(&schema, &json!({"teeth": 20, "up_axis": "stand_up", "bodies": ["b1"], "free": {"any": 1}})), Ok(()));
    let field = |value: Value| match check_input(&schema, &value) {
        Err(HostError::InvalidInput { field, .. }) => field,
        other => panic!("{value} gave {other:?}"),
    };
    assert_eq!(field(json!({})), "params.teeth");
    assert_eq!(field(json!({"teeth": 2.5})), "params.teeth");
    assert_eq!(field(json!({"teeth": 4})), "params.teeth");
    assert_eq!(field(json!({"teeth": 20, "module": 0})), "params.module");
    assert_eq!(field(json!({"teeth": 20, "name": "much too long"})), "params.name");
    assert_eq!(field(json!({"teeth": 20, "up_axis": "sideways"})), "params.up_axis");
    assert_eq!(field(json!({"teeth": 20, "bodies": [1]})), "params.bodies[0]");
    assert_eq!(field(json!({"teeth": 20, "bodies": ["a", "b", "c"]})), "params.bodies");
    assert_eq!(field(json!({"teeth": 20, "extra": true})), "params.extra", "an undeclared field is refused");
    assert_eq!(field(json!([20])), "params");
    assert_eq!(field(json!({"teeth": 20, "fixed": {"any": 1}})), "params.fixed.any", "an object schema takes only what it declares");
    // The listed schema says what the check does.
    assert_eq!(strict_input_schema(&schema)["additionalProperties"], false);
    assert_eq!(strict_input_schema(&schema)["properties"]["free"]["additionalProperties"], true);
    assert_eq!(strict_input_schema(&schema)["properties"]["fixed"]["additionalProperties"], false);
    // A hostile schema is bounded, never a crash.
    let mut deep = json!({"type":"object"});
    for _ in 0..100 {
        deep = json!({"type":"object","properties":{"a":deep}});
    }
    let mut value = json!({});
    for _ in 0..100 {
        value = json!({"a": value});
    }
    assert!(check_input(&deep, &value).is_err());
}

#[test]
fn conditional_inputs_hide_required_fields_and_reject_filtered_choices() {
    let schema = json!({"type":"object","required":["shelves"],"properties":{
        "storage":{"type":"string","enum":["shelves","drawers"]},
        "shelves":{"type":"integer","x-solidcraft-visible-when":{"field":"storage","eq":"shelves"}},
        "size":{"type":"string","oneOf":[
            {"const":"small","x-solidcraft-when":{"field":"storage","eq":"shelves"}},
            {"const":"large","x-solidcraft-when":{"not":{"field":"storage","in":["shelves"]}}}]}
    }});
    assert_eq!(check_input(&schema, &json!({"storage":"drawers","size":"large"})), Ok(()));
    assert!(matches!(check_input(&schema, &json!({"storage":"shelves"})), Err(HostError::InvalidInput { field, .. }) if field == "params.shelves"));
    assert!(
        matches!(check_input(&schema, &json!({"storage":"drawers","size":"small"})), Err(HostError::InvalidInput { field, .. }) if field == "params.size")
    );
}

#[test]
fn plain_object_scopes_skip_the_conditional_property_limit() {
    let mut properties: serde_json::Map<String, Value> = (0..1025).map(|i| (format!("p{i}"), json!({"type":"boolean"}))).collect();
    let mut value = manifest();
    value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":properties}}]);
    assert!(parse(&value).is_ok(), "plain schemas keep their existing limits");
    properties["p2"] = json!({"type":"object","properties":{
        "mode":{"type":"boolean"},"count":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}}
    }});
    value["runtime"]["entries"][0]["input_schema"]["properties"] = json!(properties);
    assert!(parse(&value).is_ok(), "nested annotations do not limit the unannotated parent scope");
    properties["p1"]["x-solidcraft-visible-when"] = json!({"field":"p0","eq":true});
    value["runtime"]["entries"][0]["input_schema"]["properties"] = json!(properties);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "input_schema.properties"));
    properties.remove("p1024");
    value["runtime"]["entries"][0]["input_schema"]["properties"] = json!(properties);
    assert!(parse(&value).is_ok(), "1,024 conditional properties remain supported");
}

#[test]
fn conditional_preparation_projects_applicable_composition_branches() {
    let branch = json!({"properties":{
        "mode":{"type":"boolean"},
        "secret":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}},
        "kept":{"type":"integer","x-solidcraft-enabled-when":{"field":"mode","eq":true}},
        "rows":{"type":"array","items":{"type":"object","properties":{
            "mode":{"type":"boolean"},"secret":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}}
        }}},
        "nested":{"type":"object","properties":{
            "mode":{"type":"boolean"},"secret":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}}
        }}
    }});
    for keyword in ["oneOf", "anyOf"] {
        let mut schema = json!({"type":"object","additionalProperties":true});
        schema[keyword] = json!([branch]);
        let input = json!({"mode":false,"secret":"must not reach Wasm","kept":3,"rows":[{"mode":false,"secret":"private"}],"nested":{"mode":false,"secret":"private"},"extra":7});
        assert_eq!(check_input(&schema, &input), Ok(()));
        assert_eq!(
            prepare_input(&schema, &input),
            Ok(json!({"mode":false,"kept":3,"rows":[{"mode":false}],"nested":{"mode":false},"extra":7})),
            "{keyword}"
        );
        let mut first = branch.clone();
        first["required"] = json!(["kind"]);
        first["properties"]["kind"] = json!({"type":"string","const":"private"});
        schema[keyword] = json!([first,{"required":["kind"],"properties":{"kind":{"type":"string","enum":["public"]},"secret":{"type":"string"}}}]);
        assert_eq!(prepare_input(&schema, &json!({"kind":"private","mode":false,"secret":"hidden"})), Ok(json!({"kind":"private","mode":false})));
        assert_eq!(prepare_input(&schema, &json!({"kind":"public","secret":"kept"})), Ok(json!({"kind":"public","secret":"kept"})));
        // Choose the branch before an enclosing scope hides its discriminator.
        schema["properties"] = json!({"mode":{"type":"boolean"},"kind":{"type":"string","x-solidcraft-visible-when":{"field":"mode","eq":true}}});
        assert_eq!(prepare_input(&schema, &json!({"kind":"private","mode":false,"secret":"hidden"})), Ok(json!({"mode":false})));
    }
}

#[test]
fn conditional_composition_shares_hidden_controllers_across_scopes() {
    for keyword in ["oneOf", "anyOf"] {
        for reverse in [false, true] {
            let controller = json!({"gate":{"type":"boolean"},"mode":{"type":"boolean","x-solidcraft-visible-when":{"field":"gate","eq":true}}});
            let dependent = json!({"mode":{"type":"boolean"},"secret":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}}});
            let (outer, inner) = if reverse { (dependent, controller) } else { (controller, dependent) };
            let mut schema = json!({"type":"object","additionalProperties":true,"properties":outer});
            schema[keyword] = json!([{"additionalProperties":true,"properties":inner}]);
            assert_eq!(
                prepare_input(&schema, &json!({"gate":false,"mode":true,"secret":7})),
                Ok(json!({"gate":false})),
                "{keyword}, reverse={reverse}"
            );
            assert_eq!(prepare_input(&schema, &json!({"gate":true,"mode":true,"secret":7})), Ok(json!({"gate":true,"mode":true,"secret":7})));
            assert_eq!(prepare_input(&schema, &json!({"gate":false,"mode":true,"secret":"hidden"})), Ok(json!({"gate":false})));
            schema["required"] = json!(["secret"]);
            schema[keyword][0]["required"] = json!(["secret"]);
            assert_eq!(check_input(&schema, &json!({"gate":false,"mode":true})), Ok(()));
            assert!(check_input(&schema, &json!({"gate":true,"mode":true})).is_err());
            let properties = if reverse { &mut schema["properties"] } else { &mut schema[keyword][0]["properties"] };
            properties["choice"] = json!({"type":"string","oneOf":[
                {"const":"shown","x-solidcraft-when":{"field":"mode","eq":true}},
                {"const":"hidden","x-solidcraft-when":{"not":{"field":"mode","eq":true}}}
            ]});
            properties["inverted"] = json!({"type":"integer","x-solidcraft-visible-when":{"not":{"field":"mode","eq":true}}});
            assert!(
                matches!(check_input(&schema, &json!({"gate":false,"mode":true,"choice":"shown"})), Err(HostError::InvalidInput { field, .. }) if field == "params.choice")
            );
            assert_eq!(
                prepare_input(&schema, &json!({"gate":false,"mode":true,"secret":"hidden","choice":"hidden","inverted":9})),
                Ok(json!({"gate":false,"choice":"hidden","inverted":9}))
            );
            let outer = json!({"type":"object","additionalProperties":true,"properties":schema["properties"]});
            let inner = schema[keyword][0].clone();
            let mut array = json!({"type":"array","items":outer});
            array[keyword] = json!([{"type":"array","items":inner}]);
            assert_eq!(prepare_input(&array, &json!([{"gate":false,"mode":true,"secret":"hidden"}])), Ok(json!([{"gate":false}])));
            let mut nested = json!({"type":"object","properties":{"nested":outer,"rows":{"type":"array","items":outer}}});
            nested[keyword] = json!([{"properties":{"nested":inner,"rows":{"type":"array","items":inner}}}]);
            assert_eq!(
                prepare_input(
                    &nested,
                    &json!({"nested":{"gate":false,"mode":true,"secret":"hidden"},"rows":[{"gate":false,"mode":true,"secret":"hidden"}]})
                ),
                Ok(json!({"nested":{"gate":false},"rows":[{"gate":false}]}))
            );
        }
    }
}

#[test]
fn conditional_composition_rejects_cross_scope_visibility_cycles_at_load() {
    let outer = json!({"type":"object","properties":{
        "a":{"type":"boolean","x-solidcraft-visible-when":{"field":"b","eq":true}},"b":{"type":"boolean"}
    }});
    let inner = json!({"type":"object","properties":{
        "a":{"type":"boolean"},"b":{"type":"boolean","x-solidcraft-visible-when":{"field":"a","eq":true}}
    }});
    for keyword in ["oneOf", "anyOf"] {
        let mut schema = outer.clone();
        schema[keyword] = json!([inner]);
        let mut nested = json!({"type":"object","properties":{"nested":outer,"rows":{"type":"array","items":outer}}});
        nested[keyword] = json!([{"properties":{"nested":inner,"rows":{"type":"array","items":inner}}}]);
        let mut array = json!({"type":"array","items":outer});
        array[keyword] = json!([{"type":"array","items":inner}]);
        let mut alternatives = schema.clone();
        alternatives[keyword][0]["required"] = json!(["kind"]);
        alternatives[keyword][0]["properties"]["kind"] = json!({"type":"string","const":"cyclic"});
        alternatives[keyword].as_array_mut().unwrap().push(json!({"required":["kind"],"properties":{"kind":{"type":"string","const":"plain"}}}));
        for candidate in [schema, nested, json!({"type":"object","properties":{"rows":array}}), alternatives] {
            let mut value = manifest();
            value["runtime"]["entries"] = json!([{"name":"make","input_schema":candidate}]);
            let result = parse(&value);
            assert!(
                matches!(&result, Err(HostError::InvalidManifest { reason, .. }) if reason.contains("cycle") && reason.contains("a") && reason.contains("b")),
                "{keyword}: {result:?}"
            );
        }
    }
}

#[test]
fn conditional_composition_keeps_exclusive_visibility_graphs_separate() {
    let branches: Vec<_> = [("left", "a", "b"), ("right", "b", "a")]
        .into_iter()
        .map(|(kind, dependent, controller)| {
            let mut branch = json!({"type":"object","required":["kind"],"properties":{
                "kind":{"type":"string","const":kind},"a":{"type":"boolean"},"b":{"type":"boolean"}
            }});
            branch["properties"][dependent]["x-solidcraft-visible-when"] = json!({"field":controller,"eq":true});
            branch
        })
        .collect();
    for keyword in ["oneOf", "anyOf"] {
        let mut schema = json!({"type":"object"});
        schema[keyword] = json!(branches);
        let mut value = manifest();
        value["runtime"]["entries"] = json!([{"name":"make","input_schema":schema}]);
        assert!(parse(&value).is_ok(), "{keyword}");
    }
}

#[test]
fn conditional_composition_rejects_ambiguous_branches_at_their_manifest_location() {
    let branch = json!({"properties":{"mode":{"type":"boolean"},"secret":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}}}});
    for keyword in ["oneOf", "anyOf", "allOf"] {
        let mut nested = json!({"type":"object"});
        nested[keyword] = if keyword == "allOf" { json!([branch]) } else { json!([branch, {}]) };
        let mut value = manifest();
        value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"nested":nested}}}]);
        let result = parse(&value);
        assert!(
            matches!(&result, Err(HostError::InvalidManifest { field, .. }) if field == &format!("input_schema.properties.nested.{keyword}")),
            "{result:?}"
        );
    }
    for (left, right, required) in [(json!(1), json!(1.0), true), (json!("a"), json!("b"), false)] {
        let branches: Vec<_> = [left, right]
            .into_iter()
            .map(|kind| {
                let mut option = branch.clone();
                option["properties"]["kind"] = json!({"const":kind});
                if required {
                    option["required"] = json!(["kind"]);
                }
                option
            })
            .collect();
        let mut value = manifest();
        value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","oneOf":branches}}]);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "input_schema.oneOf"));
    }
    // Object discriminators cannot distinguish array branches: required/properties do not
    // constrain arrays, whose item rules could otherwise hide different keys.
    let branches: Vec<_> =
        ["a", "b"].iter().map(|kind| json!({"type":"array","required":["kind"],"properties":{"kind":{"const":kind}},"items":branch})).collect();
    let mut value = manifest();
    value["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"nested":{"type":"array","oneOf":branches}}}}]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "input_schema.properties.nested.oneOf"));
}

#[test]
fn conditional_schemas_reject_missing_non_scalar_impossible_and_cyclic_references() {
    for properties in [
        json!({"a":{"type":"number","x-solidcraft-visible-when":{"field":"missing","eq":true}}}),
        json!({"a":{"type":"array"},"b":{"type":"number","x-solidcraft-enabled-when":{"field":"a","eq":[]}}}),
        json!({"a":{"type":"string","enum":["yes"]},"b":{"type":"number","x-solidcraft-defaults":[{"when":{"field":"a","eq":"no"},"value":1}]}}),
        json!({"a":{"type":"string","oneOf":[{"const":"yes"}]},"b":{"type":"string","oneOf":[{"const":"ok","x-solidcraft-when":{"field":"a","eq":"no"}}]}}),
        json!({"a":{"type":"boolean"},"b":{"type":"number","x-solidcraft-visible-when":{"field":"a","eq":"true"}}}),
        json!({"a":{"type":"boolean","x-solidcraft-visible-when":{"field":"b","eq":true}},"b":{"type":"boolean","x-solidcraft-enabled-when":{"field":"a","eq":true}}}),
    ] {
        let mut manifest = manifest();
        manifest["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":properties}}]);
        assert!(matches!(parse(&manifest), Err(HostError::InvalidManifest { .. })), "{properties}");
    }
}

#[test]
fn conditional_inputs_omit_hidden_values_and_keep_disabled_values_in_dependency_order() {
    let schema = json!({"type":"object","required":["count"],"properties":{
        "mode":{"type":"string","enum":["a","b"]},
        "child":{"type":"boolean","x-solidcraft-visible-when":{"field":"mode","eq":"a"}},
        "count":{"type":"integer","x-solidcraft-visible-when":{"all":[{"field":"child","eq":true},{"any":[{"field":"mode","in":["a"]}]}]}},
        "disabled":{"type":"integer","x-solidcraft-enabled-when":{"field":"mode","eq":"a"}}
    }});
    assert_eq!(prepare_input(&schema, &json!({"mode":"b","child":true,"count":"ignored","disabled":6})), Ok(json!({"mode":"b","disabled":6})));
    assert!(matches!(check_input(&schema, &json!({"mode":"a","child":true})), Err(HostError::InvalidInput { field, .. }) if field == "params.count"));
    let rules = conditions::InputConditions::parse(&schema).unwrap();
    assert!(!rules.fields["disabled"].enabled(json!({"mode":"b"}).as_object().unwrap()));
}

#[test]
fn conditional_schema_grammar_literals_defaults_and_work_are_bounded() {
    let property = json!({"type":"integer","enum":[1,2]});
    for condition in [
        json!({"field":"control","eq":3}),
        json!({"field":"control","eq":"1"}),
        json!({"field":"control","in":[1,"2"]}),
        json!({"all":{}}),
        json!({"not":true}),
        json!({"field":"control","eq":1,"in":[1]}),
        json!({"unknown":[]}),
    ] {
        let schema = json!({"type":"object","properties":{"control":property,"value":{"type":"number","x-solidcraft-visible-when":condition}}});
        assert!(conditions::validate_conditions(&schema).is_err(), "{condition}");
    }
    for defaults in [json!({}), json!([{"when":{"field":"control","eq":1},"value":"bad"}]), json!([{"when":{"field":"control","eq":1}}])] {
        let schema = json!({"type":"object","properties":{"control":property,"value":{"type":"number","x-solidcraft-defaults":defaults}}});
        assert!(conditions::validate_conditions(&schema).is_err());
    }
    let mut condition = json!({"field":"control","eq":1});
    for _ in 0..40 {
        condition = json!({"not":condition});
    }
    let schema = json!({"type":"object","properties":{"control":property,"value":{"type":"number","x-solidcraft-visible-when":condition}}});
    assert!(conditions::validate_conditions(&schema).is_err());
    let condition = json!({"all":vec![json!({"field":"control","eq":1});4097]});
    let schema = json!({"type":"object","properties":{"control":property,"value":{"type":"number","x-solidcraft-visible-when":condition}}});
    assert!(conditions::validate_conditions(&schema).is_err());
}

#[test]
fn conditional_numeric_values_match_gui_numbers_without_rounding_large_integers() {
    let schema = json!({"type":"object","required":["count"],"properties":{
        "mode":{"type":"number","enum":[1,9007199254740992_u64,9007199254740993_u64]},
        "count":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","in":[1,9007199254740993_u64]}}
    }});
    assert!(matches!(check_input(&schema, &json!({"mode":1.0})), Err(HostError::InvalidInput { field, .. }) if field == "params.count"));
    assert_eq!(check_input(&schema, &json!({"mode":9007199254740992.0_f64})), Ok(()));
    assert!(
        matches!(check_input(&schema, &json!({"mode":9007199254740993_u64})), Err(HostError::InvalidInput { field, .. }) if field == "params.count")
    );
}

#[test]
fn conditional_preparation_preserves_nested_objects_and_bounds_unconstrained_input_depth() {
    let schema = json!({"type":"object","properties":{
        "nested":{"type":"object","required":["count"],"properties":{
            "enabled":{"type":"boolean"},"count":{"type":"number","x-solidcraft-visible-when":{"field":"enabled","eq":true}}
        }},
        "composition":{"type":"object","properties":{"a":{},"b":{}},"oneOf":[{"properties":{"a":{"type":"integer"}}}]}
    }});
    assert!(conditions::has_conditions(&schema), "nested annotations opt into conditional execution");
    assert!(!conditions::has_conditions(&json!({"type":"object","properties":{"size":{"type":"number"}}})));
    let value = json!({"nested":{"enabled":false,"count":"ignored"},"composition":{"a":1,"b":2}});
    assert_eq!(prepare_input(&schema, &value), Ok(json!({"nested":{"enabled":false},"composition":{"a":1,"b":2}})));
    let mut deep = json!(0);
    for _ in 0..40 {
        deep = json!({"nested":deep});
    }
    assert!(prepare_input(&json!({}), &deep).is_err());
}

#[test]
fn additional_property_schemas_are_listed_as_strictly_as_they_are_checked() {
    let schema = json!({"type":"object","additionalProperties":{"type":"object","properties":{"x":{}}}});
    let listed = strict_input_schema(&schema);
    assert_eq!(listed["additionalProperties"]["additionalProperties"], false);
    for candidate in [&schema, &listed] {
        assert_eq!(check_input(candidate, &json!({"k":{"x":1}})), Ok(()));
        assert!(matches!(check_input(candidate, &json!({"k":{"x":1,"y":2}})),
            Err(HostError::InvalidInput { field, .. }) if field == "params.k.y"));
    }
}

#[test]
fn schema_composition_does_not_implicitly_close_object_constraints() {
    let schema = json!({"type":"object","allOf":[{"properties":{"a":{"type":"integer"}}}]});
    let listed = strict_input_schema(&schema);
    assert!(listed.get("additionalProperties").is_none());
    assert_eq!(check_input(&schema, &json!({"a":1})), Ok(()));
    assert_eq!(check_input(&listed, &json!({"a":1})), Ok(()));
    for keyword in ["oneOf", "anyOf"] {
        let mut schema = json!({"type":"object","properties":{"a":{},"b":{}}});
        schema[keyword] = json!([{"type":"object","required":["a"]},{"type":"object","required":["b"]}]);
        let listed = strict_input_schema(&schema);
        assert_eq!(listed["additionalProperties"], false);
        assert!(listed[keyword][0].get("additionalProperties").is_none());
        for candidate in [&schema, &listed] {
            assert_eq!(check_input(candidate, &json!({"a":1})), Ok(()));
            assert_eq!(check_input(candidate, &json!({"b":2})), Ok(()));
            assert!(check_input(candidate, &json!({})).is_err());
            assert!(check_input(candidate, &json!({"a":1,"extra":2})).is_err());
        }
        // An explicitly closed branch still refuses undeclared fields.
        schema[keyword][0]["additionalProperties"] = json!(false);
        assert!(check_input(&schema, &json!({"a":1})).is_err());
    }
}

#[test]
fn file_conditions_are_typed_host_facts_and_cannot_be_spoofed_by_options() {
    let schema = json!({"type":"object","required":["parts"],"properties":{
        "extension":{"type":"string"},
        "parts":{"type":"integer","minimum":1,"x-solidcraft-visible-when":{"all":[
            {"file":"extension","in":["asm","assembly"]},
            {"file":"format","eq":"sample.convert"},
            {"not":{"file":"size","eq":0}}
        ]}},
        "mode":{"type":"string","oneOf":[
            {"const":"part"}, {"const":"assembly","x-solidcraft-when":{"file":"extension","eq":"asm"}}
        ],"x-solidcraft-defaults":[{"when":{"file":"name","eq":"parts.asm"},"value":"assembly"}]}
    }});
    let mut file = dialog::FileFacts { name: "parts.asm".into(), extension: "asm".into(), size: 24, format: "sample.convert".into(), generation: 1 };
    conditions::validate_conditions(&schema).unwrap();
    assert!(conditions::uses_file_facts(&schema));
    let bound = conditions::bind_file_conditions(&schema, Some(&file)).unwrap();
    assert!(check_input(&bound, &json!({"extension":"txt"})).is_err(), "parts is required for the actual file");
    assert!(check_input(&bound, &json!({"parts":2,"mode":"assembly"})).is_ok());
    file.extension = "txt".into();
    let bound = conditions::bind_file_conditions(&schema, Some(&file)).unwrap();
    assert_eq!(prepare_input(&bound, &json!({"extension":"asm","parts":"hidden"})).unwrap(), json!({"extension":"asm"}));
    assert!(check_input(&bound, &json!({"mode":"assembly"})).is_err());
    for condition in [
        json!({"file":"path","eq":"/secret"}),
        json!({"file":"size","eq":-1}),
        json!({"file":"extension","eq":1}),
        json!({"file":"generation","eq":1}),
        json!({"file":"name","eq":"x","field":"parts"}),
    ] {
        let bad = json!({"type":"object","properties":{"part":{"type":"string","x-solidcraft-visible-when":condition}}});
        assert!(conditions::validate_conditions(&bad).is_err());
    }
}

#[test]
fn dialog_field_updates_are_bounded_and_preserve_types_conditions_and_directory_semantics() {
    let schema = json!({"type":"object","properties":{
        "part":{"type":"string"},
        "folder":{"type":"string","format":"directory"},
        "count":{"type":"integer","minimum":1}
    }});
    let update = json!({"fields":{"part":{"oneOf":[{"const":"plate","title":"Plate"},{"const":"cover","title":"Cover"}],"default":"plate"}}});
    let updated = dialog::apply_update(&schema, &update).unwrap();
    assert_eq!(updated["properties"]["part"]["default"], "plate");
    assert_eq!(updated["properties"]["folder"], schema["properties"]["folder"]);
    for fields in [
        json!({"undeclared":{"default":1}}),
        json!({"part":{"type":"integer"}}),
        json!({"folder":{"format":"text"}}),
        json!({"folder":{"enum":["/etc"]}}),
        json!({"count":{"default":0}}),
        json!({"part":{"oneOf":[{"const":"plate","x-solidcraft-when":{"all":[]}}]}}),
        json!({"part":{"enum":[]}}),
        json!({"part":{"enum":[4]}}),
        json!({"part":{"description":"x".repeat(dialog::MAX_UPDATE_BYTES)}}),
    ] {
        assert!(dialog::apply_update(&schema, &json!({"fields":fields})).is_err());
    }
    let mut importing = importer();
    importing["capabilities"] = json!(["formats@0.2", "dialog@0.1"]);
    assert!(parse_manifest(importing.to_string().as_bytes(), "sample").is_ok());
    let mut command = manifest();
    command["capabilities"] = json!(["commands@0.2", "dialog@0.1"]);
    command["runtime"]["entries"][0] = json!({"name":"make","dialog_hook":true});
    assert!(parse_manifest(command.to_string().as_bytes(), "sample").is_ok());
    command["capabilities"] = json!(["commands@0.2"]);
    assert!(parse_manifest(command.to_string().as_bytes(), "sample").is_err());
}

#[test]
fn folder_read_annotations_are_explicit_bounded_and_only_on_top_level_directories() {
    let directory = json!({"type":"string","format":"directory","x-solidcraft-grant":"read"});
    let schema = json!({"type":"object","properties":{"library":directory}});
    assert_eq!(directory_read_fields(&schema).unwrap(), BTreeSet::from(["library".into()]));
    let mut value = manifest();
    value["capabilities"] = json!(["commands@0.2", "folders@0.1"]);
    value["runtime"]["entries"] = json!([{"name":"make","input_schema":schema}]);
    assert!(parse(&value).is_ok());
    for property in [
        json!({"type":"string","format":"directory","x-solidcraft-grant":"write"}),
        json!({"type":"string","x-solidcraft-grant":"read"}),
        json!({"type":"integer","format":"directory","x-solidcraft-grant":"read"}),
        json!({"type":"string","format":"directory","x-solidcraft-grant":true}),
        json!({"type":"object","properties":{"nested":directory}}),
        json!({"type":"array","items":directory}),
    ] {
        value["runtime"]["entries"][0]["input_schema"] = json!({"type":"object","properties":{"library":property}});
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })), "{property}");
    }
    assert!(directory_read_fields(&json!({"type":"object","oneOf":[schema]})).is_err());
    let properties: serde_json::Map<_, _> = (0..33).map(|i| (format!("folder{i}"), directory.clone())).collect();
    assert!(directory_read_fields(&json!({"type":"object","properties":properties})).is_err());
    assert!(directory_read_fields(&json!({"type":"object","properties":{"old":{"type":"string","format":"directory"}}})).unwrap().is_empty());
}

#[test]
fn folder_read_annotations_under_other_schema_keywords_are_rejected_at_manifest_load() {
    let directory = json!({"type":"string","format":"directory","x-solidcraft-grant":"read"});
    let plain_directory = json!({"type":"string","format":"directory"});
    for keyword in ["additionalItems", "contains", "contentSchema", "dependencies", "propertyNames", "unevaluatedItems", "unevaluatedProperties"] {
        for top_level_grant in [false, true] {
            let mut schema = json!({"type":"object","properties":{}});
            if top_level_grant {
                schema["properties"]["library"] = directory.clone();
            }
            schema[keyword] = if keyword == "dependencies" { json!({"mode":directory}) } else { directory.clone() };
            let error = directory_read_fields(&schema).unwrap_err();
            assert!(matches!(error, HostError::InvalidManifest { ref field, .. } if field.contains(keyword)), "{keyword}: {error}");
            let mut value = manifest();
            value["runtime"]["entries"] = json!([{"name":"make","input_schema":schema}]);
            assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })), "{keyword}: {top_level_grant}");

            schema[keyword] = if keyword == "dependencies" { json!({"mode":plain_directory}) } else { plain_directory.clone() };
            let expected = if top_level_grant { BTreeSet::from(["library".into()]) } else { BTreeSet::new() };
            assert_eq!(directory_read_fields(&schema).unwrap(), expected, "{keyword}: {top_level_grant}");
            value["runtime"]["entries"][0]["input_schema"] = schema;
            assert!(parse(&value).is_ok(), "{keyword}: {top_level_grant}");
        }
    }
}

#[test]
fn an_importer_may_choose_its_file_first_and_old_manifests_keep_their_shape() {
    let mut value = importer();
    let old = parse(&value).unwrap().runtime.unwrap();
    assert!(!old.entries[0].file_first, "absent keeps the options-first dialog");
    let old_shape = serde_json::to_value(&old.entries[0]).unwrap();
    assert!(old_shape.get("file_first").is_none(), "an importer without it serializes as before: {old_shape}");
    value["runtime"]["entries"][0]["file_first"] = json!(true);
    let entry = parse(&value).unwrap().runtime.unwrap().entries.remove(0);
    assert!(entry.file_first);
    let saved = serde_json::to_value(&entry).unwrap();
    assert_eq!(saved["file_first"], true);
    let again: Entry = serde_json::from_value(saved.clone()).unwrap();
    assert!(again.file_first && serde_json::to_value(&again).unwrap() == saved, "it round-trips");
    value["runtime"]["entries"][0]["file_first"] = json!(false);
    let explicit = parse(&value).unwrap().runtime.unwrap().entries.remove(0);
    assert!(!explicit.file_first && serde_json::to_value(&explicit).unwrap() == old_shape);
    // A hostile value is refused: only a boolean, and only an importer.
    for hostile in [json!("yes"), json!(1), json!(null), json!([true]), json!({"on":true})] {
        let mut bad = importer();
        bad["runtime"]["entries"][0]["file_first"] = hostile.clone();
        assert!(matches!(parse(&bad), Err(HostError::InvalidManifest { .. })), "{hostile}");
    }
    let mut command = manifest();
    command["runtime"]["entries"] = json!([{"name":"make","file_first":true}]);
    assert!(matches!(parse(&command), Err(HostError::InvalidManifest { field, .. }) if field.contains("file_first")));
    let mut exporting = exporter();
    exporting["runtime"]["entries"][0]["file_first"] = json!(true);
    assert!(matches!(parse(&exporting), Err(HostError::InvalidManifest { field, .. }) if field.contains("file_first")));
}

#[test]
fn declared_property_order_follows_shared_definitions_at_the_reference() {
    let bytes = br##"{"id":"sample","name":"Sample","version":"1","capabilities":["commands@0.1"],
        "$defs":{
            "pair":{"type":"object","properties":{"zz":{"type":"number"},"aa":{"type":"number"}}},
            "size":{"type":"number","default":1}
        },
        "runtime":{"module":"plugin.wasm","commands":[],"entries":[{"name":"make","input_schema":{
            "type":"object","x-solidcraft-field-order":"declared","properties":{
                "zeta":{"$ref":"#/$defs/size"},
                "mid":{"$ref":"#/$defs/pair"},
                "own":{"$ref":"#/$defs/pair","properties":{"y":{"type":"number"},"b":{"type":"number"}}},
                "alpha":{"type":"boolean","default":false}
            }
        }}]}}"##;
    let manifest = parse_manifest(bytes, "sample").unwrap();
    let entry = &manifest.runtime.as_ref().unwrap().entries[0];
    assert_eq!(entry.input_property_order[""], ["zeta", "mid", "own", "alpha"]);
    assert_eq!(entry.input_property_order["/properties/mid"], ["zz", "aa"]);
    assert_eq!(entry.input_property_order["/properties/own"], ["y", "b"]);
    assert_eq!(entry.input_schema["properties"]["mid"]["properties"]["aa"]["type"], "number");
    assert!(!serde_json::to_string(&manifest).unwrap().contains("internal-property-order"), "the order marker never leaks");
    // The recorded order survives the host's own round trip.
    let round_trip = parse_manifest(&serde_json::to_vec(&manifest).unwrap(), "sample").unwrap();
    assert_eq!(round_trip.runtime.unwrap().entries[0].input_property_order, entry.input_property_order);
}

#[test]
fn updates_name_a_github_repository() {
    let mut value = manifest();
    value["updates"] = json!({"github": "maker-1/gear_tools.sc"});
    assert_eq!(parse(&value).unwrap().updates, Some(Updates { github: "maker-1/gear_tools.sc".into() }));
    for bad in ["maker", "maker/", "/gears", "-maker/gears", "maker/gears/x", "maker/..", "maker/gears?tab=1", "https://github.com/maker/gears"] {
        value["updates"] = json!({"github": bad});
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field == "updates.github"), "{bad}");
    }
    value["updates"] = json!({"github": "maker/gears", "url": "https://example.com/feed"});
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
    assert!(parse(&manifest()).unwrap().updates.is_none());
}

#[test]
fn theme_manifests_are_checked() {
    let themed =
        |themes: Value, capabilities: Value| json!({"id":"sample","name":"Sample","version":"1","capabilities":capabilities,"themes":themes});
    let good = json!([{"id":"solid-glass","source":"solid-glass.json"},{"id":"night","source":"night.json"}]);
    let plugin = parse(&themed(good.clone(), json!(["themes@0.1"]))).unwrap();
    assert_eq!(plugin.themes.len(), 2);
    assert_eq!(plugin.themes.first().map(|theme| theme.source.as_str()), Some("solid-glass.json"));

    assert!(matches!(parse(&themed(good.clone(), json!([]))), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    assert!(matches!(parse(&themed(good.clone(), json!(["themes@0.2"]))), Err(HostError::UnsupportedCapability { .. })));
    assert!(parse(&themed(json!([{"id":"a","source":"a.json"},{"id":"a","source":"b.json"}]), json!(["themes@0.1"]))).is_err());
    for source in ["../x.json", "x.wgsl", "Big.json", "dir/x.json", "x.JSON", "-x.json"] {
        assert!(parse(&themed(json!([{"id":"a","source":source}]), json!(["themes@0.1"]))).is_err(), "{source}");
    }
    let many: Vec<Value> = (0..9).map(|i| json!({"id": format!("t{i}"), "source": format!("t{i}.json")})).collect();
    assert!(parse(&themed(json!(many), json!(["themes@0.1"]))).is_err());
    assert!(matches!(parse(&themed(json!([]), json!(["themes@0.1"]))), Err(HostError::InvalidManifest { field, .. }) if field == "themes"));
}

#[test]
fn theme_files_must_be_theme_json() {
    let good = br#"{"format":"solidcraft-theme/1","name":"Night","base":"dark"}"#;
    assert!(theme::check_file("night", good).is_ok());
    assert!(theme::check_file("night", br#"{"format":"solidcraft-theme/2","name":"Night"}"#).is_err());
    assert!(theme::check_file("night", br#"{"format":"solidcraft-theme/1"}"#).is_err());
    assert!(theme::check_file("night", br#"["solidcraft-theme/1"]"#).is_err());
    assert!(theme::check_file("night", b"not json").is_err());
    let big = format!(r#"{{"format":"solidcraft-theme/1","name":"N","pad":"{}"}}"#, "x".repeat(theme::MAX_THEME_BYTES));
    assert!(matches!(theme::check_file("night", big.as_bytes()), Err(HostError::LimitExceeded { .. })));
}

#[test]
fn a_manifest_without_themes_reads_and_writes_as_before() {
    let legacy = manifest();
    let parsed = parse(&legacy).unwrap();
    assert!(parsed.themes.is_empty());
    assert_eq!(parsed.capabilities, ["commands@0.1"]);
    let written = serde_json::to_value(&parsed).unwrap();
    assert!(written.get("themes").is_none());
    assert_eq!(written["capabilities"], json!(["commands@0.1"]));
    assert_eq!(written["runtime"]["module"], "plugin.wasm");
    assert!(parse(&json!({"id":"sample","name":"Sample","version":"1","themes":[{"id":"a","source":"a.json"}]})).is_err());
}
