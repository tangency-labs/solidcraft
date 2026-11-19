//! The Plugins menu and its entry dialog: schema fields, selection fields picked in the viewport,
//! required gating, and a run through the dialog matching `plugin.run`.
use super::*;
use std::collections::BTreeSet;

fn gear_schema() -> Value {
    json!({"type":"object","required":["module","teeth"],"properties":{
        "module":{"type":"number","title":"Module","description":"Tooth size","unit":"mm","exclusiveMinimum":0,"maximum":50,"default":2},
        "teeth":{"type":"integer","title":"Teeth","minimum":6,"maximum":400,"default":20},
        "pressure_angle":{"type":"string","title":"Pressure angle","enum":["14.5","20","25"],"default":"20"},
        "hub":{"type":"boolean","title":"Hub"},
        "name":{"type":"string","maxLength":8},
        "bore":{"type":"number","minimum":0}
    }})
}

#[test]
fn each_schema_type_becomes_its_field_with_limits_labels_and_defaults() {
    let fields = form(&gear_schema()).expect("drawn");
    let keys: Vec<&str> = fields.iter().map(|(f, _)| f.key.as_str()).collect();
    assert_eq!(keys, ["module", "teeth", "bore", "hub", "name", "pressure_angle"], "required first, in order; then by name");
    let (module, input) = &fields[0];
    assert_eq!((module.label.as_str(), module.tip.as_deref(), module.required), ("Module", Some("Tooth size"), true));
    assert_eq!(
        module.kind,
        Kind::Number { min: Some(0.0), max: Some(50.0), exclusive_min: true, exclusive_max: false, step: None, unit: Some("mm".into()) }
    );
    assert_eq!(*input, Input::Number(2.0));
    assert_eq!(fields[1].0.kind, Kind::Integer { min: Some(6), max: Some(400), unit: None });
    assert_eq!(fields[1].1, Input::Integer(20));
    assert_eq!(fields[2].1, Input::Typed(String::new()), "a number without a default starts empty");
    assert_eq!((&fields[3].0.kind, &fields[3].1), (&Kind::Boolean, &Input::Boolean(false)));
    assert_eq!(fields[4].0.label, "name", "no title: the key");
    assert_eq!(fields[4].0.kind, Kind::Text { min_length: None, max_length: Some(8) });
    let angles: Vec<String> = vec!["14.5".into(), "20".into(), "25".into()];
    assert_eq!(fields[5].0.kind, Kind::Choice { values: angles.clone(), titles: angles }, "enum: each value shown as it is");
    assert_eq!(fields[5].1, Input::Choice(Some(1)), "the default");
    let no_default = form(&json!({"type":"object","properties":{"p":{"type":"string","enum":["a","b"]}}})).unwrap();
    assert_eq!(no_default[0].1, Input::Choice(None), "no default: nothing chosen yet");
    assert_eq!(form(&json!({"type":"object"})), Some(Vec::new()), "no properties: nothing to fill");
}

#[test]
fn choice_options_show_their_titles_and_send_their_values() {
    let schema = json!({"type":"object","properties":{"up":{"type":"string","title":"Up axis","default":"as_is","oneOf":[
        {"const":"as_is","title":"As in the file"},{"const":"stand_up","title":"Stand up (Y-up to Z-up)","description":"Turns the design"},{"const":"raw"}
    ]}}});
    let mut fields = form(&schema).expect("drawn");
    assert_eq!(
        fields[0].0.kind,
        Kind::Choice {
            values: vec!["as_is".into(), "stand_up".into(), "raw".into()],
            titles: vec!["As in the file".into(), "Stand up (Y-up to Z-up)".into(), "raw".into()]
        },
        "an option without a title shows its value"
    );
    assert_eq!(fields[0].1, Input::Choice(Some(0)), "the default, by value");
    fields[0].1 = Input::Choice(Some(1));
    assert_eq!(params(&fields), Ok(json!({"up":"stand_up"})), "the value goes to the plugin, not the title");
    for options in [
        json!([]),
        json!([{"const":1,"title":"One"}]),
        json!([{"const":"a"},{"const":"a","title":"Again"}]),
        json!([{"const":"a","pattern":"x"}]),
        json!([{"title":"No value"}]),
        json!(["a"]),
    ] {
        assert_eq!(form(&json!({"type":"object","properties":{"p":{"type":"string","oneOf":options}}})), None, "{options}");
    }
    assert_eq!(form(&json!({"type":"object","properties":{"p":{"type":"string","enum":["a"],"oneOf":[{"const":"a"}]}}})), None, "not both");
}

#[test]
fn other_lists_and_nested_objects_are_not_guessed() {
    for property in [
        // A target list of strings: not body names (only `bodies` and `body_ids` send those).
        json!({"type":"array","items":{"type":"string"}}),
        json!({"type":"object","properties":{"x":{"type":"number"}}}),
        json!({"$ref":"#/definitions/face"}),
        json!({"oneOf":[{"type":"number"},{"type":"string"}]}),
        json!({"type":["number","null"]}),
        json!({"type":"number","enum":[1,2]}),
        json!({"description":"no type"}),
    ] {
        assert_eq!(form(&json!({"type":"object","properties":{"target":property}})), None, "{property}");
    }
    assert_eq!(form(&json!({"type":"array"})), None);
    assert_eq!(form(&json!({"type":"object","required":["missing"],"properties":{}})), None);
}

/// A schema with two selection fields: the edges to work on, and one body.
fn pick_schema() -> Value {
    json!({"type":"object","required":["edges","body"],"properties":{
        "edges":{"type":"array","title":"Edges","description":"The rims to seat","minItems":1,
            "x-solidcraft-selection":{"version":1,"accept":["edge"]}},
        "body":{"type":"array","title":"Body to modify","maxItems":1,"items":{"type":"object"},
            "x-solidcraft-selection":{"version":1,"accept":["body"]}},
        "depth":{"type":"number","default":2}
    }})
}

#[test]
fn annotated_and_reserved_lists_become_selection_fields() {
    use crate::selection::{BODIES, EDGES, EXTERNAL_POINTS, FACES, PROFILES, SKETCH_LINES};
    let fields = form(&pick_schema()).expect("drawn");
    let kinds: Vec<(&str, &Kind)> = fields.iter().map(|(f, _)| (f.key.as_str(), &f.kind)).collect();
    assert_eq!(kinds[0], ("edges", &Kind::Selection { accept: EDGES, min: 1, max: None, names: false }));
    assert_eq!(kinds[1], ("body", &Kind::Selection { accept: BODIES, min: 0, max: Some(1), names: false }));
    assert_eq!((fields[0].0.label.as_str(), fields[0].0.tip.as_deref()), ("Edges", Some("The rims to seat")));
    assert_eq!(fields[0].1, Input::Selection(Vec::new()), "nothing picked yet");
    let every = json!({"type":"object","properties":{"p":{"type":"array",
        "x-solidcraft-selection":{"version":1,"accept":["profile","edge","face","body","sketch_point","sketch_curve"]}}}});
    assert!(
        matches!(form(&every).unwrap()[0].0.kind, Kind::Selection { accept, .. } if accept == PROFILES | EDGES | FACES | BODIES | EXTERNAL_POINTS | SKETCH_LINES),
        "each category is its pick kind"
    );
    // The lists the manifest reserves are selection fields without the annotation.
    let reserved = |key: &str, items: Value| {
        let mut property = json!({"type":"array"});
        if !items.is_null() {
            property["items"] = items;
        }
        form(&json!({"type":"object","properties":{key:property}})).map(|f| f[0].0.kind.clone())
    };
    for key in ["bodies", "body_ids"] {
        assert_eq!(reserved(key, Value::Null), Some(Kind::Selection { accept: BODIES, min: 0, max: None, names: true }), "{key}");
        assert!(reserved(key, json!({"type":"string"})).is_some(), "{key}: names are strings");
        assert_eq!(reserved(key, json!({"type":"object"})), None, "{key}: not references");
    }
    for key in ["selection", "target", "targets"] {
        assert_eq!(reserved(key, Value::Null), Some(Kind::Selection { accept: EDGES | FACES | BODIES, min: 0, max: None, names: false }), "{key}");
    }
    assert_eq!(reserved("points", Value::Null), None, "any other list is not guessed");
}

#[test]
fn a_selection_annotation_this_host_cannot_honour_is_not_guessed() {
    let one = |property: Value| form(&json!({"type":"object","properties":{"p":property}}));
    let annotated = |annotation: Value| json!({"type":"array","x-solidcraft-selection":annotation});
    for property in [
        annotated(json!({"version":2,"accept":["edge"]})),
        annotated(json!({"accept":["edge"]})),
        annotated(json!({"version":1,"accept":["edge","hole"]})),
        annotated(json!({"version":1,"accept":[]})),
        annotated(json!({"version":1,"accept":"edge"})),
        annotated(json!({"version":1,"accept":["edge"],"tangent_chain":true})),
        json!({"type":"array","minItems":2,"maxItems":1,"x-solidcraft-selection":{"version":1,"accept":["edge"]}}),
        json!({"type":"array","maxItems":0,"x-solidcraft-selection":{"version":1,"accept":["edge"]}}),
        json!({"type":"array","uniqueItems":true,"x-solidcraft-selection":{"version":1,"accept":["edge"]}}),
        json!({"type":"array","items":{"type":"string"},"x-solidcraft-selection":{"version":1,"accept":["edge"]}}),
        json!({"type":"array","items":{"type":"object","required":["body"]},"x-solidcraft-selection":{"version":1,"accept":["edge"]}}),
        json!({"type":"array","title":3,"x-solidcraft-selection":{"version":1,"accept":["edge"]}}),
    ] {
        assert_eq!(one(property.clone()), None, "{property}");
    }
}

#[test]
fn picks_are_sent_as_explicit_references_and_counted() {
    use solidcore_record::Vec3;
    let mut fields = form(&pick_schema()).unwrap();
    assert_eq!(params(&fields), Err(Problem::Missing("Edges".into())), "minItems 1: a pick is needed");
    let edge = Sel::Edge { body: "Body1".into(), index: 3, point: Vec3::new(1.0, 2.0, 3.0) };
    fields[0].1 = Input::Selection(vec![edge.clone()]);
    let sent = params(&fields).unwrap();
    assert_eq!(sent["edges"], json!([serde_json::to_value(&edge).unwrap()]), "the engine's own selection reference");
    assert_eq!(sent["edges"][0]["type"], "edge");
    assert_eq!(sent["body"], json!([]), "required with no minimum: sent, empty");
    fields[1].1 = Input::Selection(vec![Sel::Body { name: "A".into() }, Sel::Body { name: "B".into() }]);
    assert_eq!(params(&fields), Err(Problem::Invalid("Body to modify".into())), "more than maxItems");
    fields[1].1 = Input::Selection(vec![Sel::Body { name: "A".into() }]);
    assert_eq!(params(&fields).unwrap()["body"], json!([{"type":"body","name":"A"}]));
    // An optional field with nothing picked is left out; `bodies` sends names.
    let mut bodies = form(&json!({"type":"object","properties":{"bodies":{"type":"array"}}})).unwrap();
    assert_eq!(params(&bodies), Ok(json!({})));
    bodies[0].1 = Input::Selection(vec![Sel::Body { name: "Body1".into() }]);
    assert_eq!(params(&bodies), Ok(json!({"bodies":["Body1"]})));
}

#[test]
fn constraints_the_dialog_cannot_check_are_not_guessed() {
    let one = |property: Value| form(&json!({"type":"object","properties":{"p":property}}));
    for property in [
        json!({"type":"string","pattern":"^[A-Z]+$"}),
        json!({"type":"string","contentEncoding":"base64"}),
        json!({"type":"number","minimum":"5"}),
        json!({"type":"number","multipleOf":0}),
        json!({"type":"number","minimum":5,"maximum":1}),
        json!({"type":"integer","multipleOf":2}),
        json!({"type":"string","maxLength":-1}),
        json!({"type":"boolean","readOnly":true}),
    ] {
        assert_eq!(one(property.clone()), None, "{property}");
    }
    assert!(one(json!({"type":"string","format":"email","examples":["a@b.c"]})).is_some(), "annotations are fine");
    assert_eq!(form(&json!({"type":"object","required":["key"]})), None, "a required key with no field to fill it");
    assert_eq!(form(&json!({"type":"object","required":[]})), Some(Vec::new()));
}

#[test]
fn integer_bounds_apply_every_form_and_refuse_what_does_not_fit_i64() {
    let bounds = |property: Value| {
        form(&json!({"type":"object","properties":{"n":property}})).map(|f| match &f[0].0.kind {
            Kind::Integer { min, max, .. } => (*min, *max),
            other => panic!("{other:?}"),
        })
    };
    assert_eq!(bounds(json!({"type":"integer","minimum":0,"exclusiveMinimum":true})), Some((Some(1), None)), "draft-4 boolean");
    assert_eq!(bounds(json!({"type":"integer","maximum":10,"exclusiveMaximum":true})), Some((None, Some(9))));
    assert_eq!(bounds(json!({"type":"integer","minimum":3,"exclusiveMinimum":5})), Some((Some(6), None)), "both apply");
    assert_eq!(bounds(json!({"type":"integer","minimum":2.2,"exclusiveMaximum":5.5})), Some((Some(3), Some(5))));
    assert_eq!(bounds(json!({"type":"integer","exclusiveMinimum":1e30})), None, "outside i64");
    assert_eq!(bounds(json!({"type":"integer","maximum":-1e30})), None);
    assert_eq!(bounds(json!({"type":"integer","exclusiveMaximum":-9223372036854775808.0_f64})), None, "no i64 below the lowest");
    assert_eq!(bounds(json!({"type":"integer","minimum":9223372036854775807_i64})), None, "rounds to 2^63, past i64");
    assert_eq!(bounds(json!({"type":"integer","minimum":5,"maximum":4})), None, "no value fits");
}

#[test]
fn run_waits_for_required_fields_and_valid_values() {
    let schema = json!({"type":"object","required":["teeth","profile","label","bore"],"properties":{
        "teeth":{"type":"integer","minimum":6},
        "profile":{"type":"string","enum":["spur","helical"]},
        "label":{"type":"string"},
        "bore":{"type":"number","exclusiveMinimum":0},
        "note":{"type":"string"}}});
    let mut fields = form(&schema).unwrap();
    let set = |fields: &mut Vec<(Field, Input)>, key: &str, input: Input| {
        fields.iter_mut().find(|(f, _)| f.key == key).unwrap().1 = input;
    };
    assert_eq!(params(&fields), Err(Problem::Missing("teeth".into())));
    set(&mut fields, "teeth", Input::Typed("five".into()));
    assert_eq!(params(&fields), Err(Problem::Invalid("teeth".into())));
    set(&mut fields, "teeth", Input::Typed("5".into()));
    assert_eq!(params(&fields), Err(Problem::Invalid("teeth".into())), "below its minimum");
    set(&mut fields, "teeth", Input::Typed("12".into()));
    assert_eq!(params(&fields), Err(Problem::Missing("profile".into())), "Choose… is not a choice");
    set(&mut fields, "profile", Input::Choice(Some(1)));
    assert_eq!(params(&fields), Err(Problem::Missing("label".into())));
    set(&mut fields, "label", Input::Text("G1".into()));
    set(&mut fields, "bore", Input::Typed("0".into()));
    assert_eq!(params(&fields), Err(Problem::Invalid("bore".into())), "exclusive minimum");
    set(&mut fields, "bore", Input::Typed("5".into()));
    assert_eq!(
        params(&fields),
        Ok(json!({"teeth":12,"profile":"helical","label":"G1","bore":5.0})),
        "an empty optional field is left to the plugin's default"
    );
}

fn entry(name: &str, effect: &str, slots: Value) -> solidcraft_engine::plugin::Entry {
    serde_json::from_value(json!({"name":name,"effect":effect,"input_schema":{"type":"object"},"slots":slots})).unwrap()
}

fn listed(id: &str, entries: Vec<solidcraft_engine::plugin::Entry>) -> Listed {
    Listed {
        id: id.into(),
        name: format!("{id} plugin"),
        version: "0.1.0".into(),
        description: String::new(),
        path: String::new(),
        disabled: false,
        approval: String::new(),
        groups: BTreeSet::new(),
        entries,
        images: Default::default(),
    }
}

#[test]
fn a_numeric_plugin_editor_interrupts_external_joint_drive() {
    use crate::control::tests::{assert_local_editor_interrupts_joint_drive, joint_drive_design};

    let (mut h, arm, _) = joint_drive_design();
    let mut make = entry("make", "edit", json!([]));
    make.input_schema = json!({"type":"object","properties":{"length":{"type":"number","default":20}}});
    h.app.plugin_menu.listed = vec![listed("boxes", vec![make])];
    h.app.plugin_menu.loaded = true;
    assert_local_editor_interrupts_joint_drive(&mut h, arm, |app| {
        open_entry(app, 0, 0);
        assert!(app.dialog.is_none(), "the editor has no viewport pick field");
        assert_eq!(app.plugin_menu.dialog().unwrap().ready(app), Ok(json!({"length":20.0})));
        assert!(preview_commands(app).is_some());
    });
}

#[test]
fn the_menu_lists_install_then_entries_with_a_submenu_for_several() {
    use crate::app_bar::FileRow;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let empty = rows(&app);
    assert!(matches!(&empty[0], FileRow::Item(i) if i.id == INSTALL));
    assert!(matches!(&empty[1], FileRow::Item(i) if i.id == LAST_REPORT && !i.enabled), "no import yet: nothing to show again");
    assert!(matches!(&empty[2], FileRow::Separator));
    assert!(matches!(&empty[3], FileRow::Item(i) if i.label == app.tr("ui.plugin_menu_empty") && !i.enabled));
    app.plugin_menu.listed = vec![
        listed("gears", vec![entry("spur", "edit", json!([{"kind":"menu","name":"Spur Gear"}]))]),
        listed("tools", vec![entry("measure", "read", json!([])), entry("tidy", "edit", json!([{"kind":"ribbon","name":"x"}]))]),
    ];
    let listed_rows = rows(&app);
    assert_eq!(listed_rows.len(), 5);
    assert!(matches!(&listed_rows[3], FileRow::Item(i) if i.id == "entry:0:0" && i.label == "Spur Gear…"), "menu slot names the row");
    let FileRow::Sub { label, rows: sub } = &listed_rows[4] else { panic!("several entries: a submenu") };
    assert_eq!(label, "tools plugin");
    let labels: Vec<&str> = sub.iter().map(|r| if let FileRow::Item(i) = r { i.label.as_str() } else { "" }).collect();
    assert_eq!(labels, ["measure…", "tidy…"]);
}

/// Each plugin is its own thing: every command gets its own menu row (several under the plugin's
/// name) and its own ribbon button, and a plugin turned off in Preferences shows in neither.
#[test]
fn each_plugin_command_has_its_own_menu_row_and_ribbon_button_and_a_disabled_plugin_is_hidden() {
    use crate::app_bar::FileRow;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.plugin_menu.listed = vec![
        listed("gears", vec![entry("spur", "edit", json!([]))]),
        listed("tools", vec![entry("measure", "read", json!([])), entry("tidy", "edit", json!([{"kind":"ribbon","name":"Tidy up"}]))]),
    ];
    let buttons = ribbon_buttons(&app);
    let ids: Vec<&str> = buttons.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["plugin_entry:gears:spur", "plugin_entry:tools:measure", "plugin_entry:tools:tidy"]);
    let labels: Vec<&str> = buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["gears plugin", "measure", "Tidy up"], "one command: the plugin's name; a ribbon slot names its button");
    assert!(buttons.iter().all(|b| b.icon == "prefs_plugins"), "no manifest icon yet: the generic plugin icon");
    assert!(app.plugin_menu.dialogs.is_empty(), "listing opens nothing");

    let menu = rows(&app);
    assert!(matches!(&menu[3], FileRow::Item(i) if i.id == "entry:0:0" && i.label == "gears plugin…"));
    assert!(matches!(&menu[4], FileRow::Sub { label, rows } if label == "tools plugin" && rows.len() == 2));

    // The ribbon button opens that one command's dialog, titled by it alone.
    assert!(choose_ribbon(&mut app, "plugin_entry:tools:tidy"));
    let dialog = app.plugin_menu.dialogs.pop().expect("its dialog");
    assert_eq!((dialog.plugin.id.as_str(), dialog.entry.as_str(), dialog.label.as_str()), ("tools", "tidy", "tidy"));
    assert!(!choose_ribbon(&mut app, "sketch.line"), "a command's id is not a plugin's");

    // The ribbon group's label opens the same menu below it (a narrow ribbon trims buttons).
    let label = egui::Rect::from_min_size(egui::pos2(900.0, 140.0), egui::vec2(60.0, 16.0));
    let listed = app.plugin_menu.listed.clone();
    toggle_below(&mut app, &egui::Context::default(), label);
    assert!(app.plugin_menu.open && app.plugin_menu.y == Some(156.0) && app.plugin_menu.x == 900.0);
    app.plugin_menu.open = false;
    // Opening reads the plugins folder again; this test's plugins are not on disk.
    app.plugin_menu.listed = listed;

    app.preferences.disabled_plugins.insert("gears".into());
    let ids: Vec<String> = ribbon_buttons(&app).into_iter().map(|b| b.id).collect();
    assert_eq!(ids, ["plugin_entry:tools:measure", "plugin_entry:tools:tidy"], "off: no ribbon button");
    let menu = rows(&app);
    assert_eq!(menu.len(), 4, "off: no menu row");
    assert!(matches!(&menu[3], FileRow::Sub { label, .. } if label == "tools plugin"));
    choose_ribbon(&mut app, "plugin_entry:gears:spur");
    choose(&mut app, "entry:0:0");
    assert!(app.plugin_menu.dialogs.is_empty(), "a plugin turned off opens nothing");

    app.preferences.disabled_plugins.insert("tools".into());
    assert!(ribbon_buttons(&app).is_empty());
    assert!(matches!(&rows(&app)[3], FileRow::Item(i) if i.label == app.tr("ui.plugin_menu_all_off") && !i.enabled));
}

#[test]
fn plugin_dialog_titles_are_the_command_alone_and_plugin_text_has_no_long_dashes() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let mut gears = listed("gears", vec![entry("spur", "edit", json!([]))]);
    gears.name = "Gears".into();
    app.plugin_menu.listed = vec![gears];
    open_entry(&mut app, 0, 0);
    assert_eq!(app.plugin_menu.dialog().map(|d| d.label.as_str()), Some("Gears"), "no \"- Plugins\" after it");
    // Every plugin string the user can see, in English and in the sources that draw them.
    let en: serde_json::Map<String, Value> = serde_json::from_str(include_str!("../../../assets/locales/en.json")).expect("en.json");
    for (key, text) in en.iter().filter(|(k, _)| k.contains("plugin")) {
        let text = text.as_str().unwrap_or_default();
        assert!(!text.contains(['\u{2014}', '\u{2013}']), "{key}: {text}");
    }
    for (file, source) in [
        ("plugin_menu.rs", include_str!("plugin_menu.rs")),
        ("plugins.rs", include_str!("plugins.rs")),
        ("plugin_install.rs", include_str!("plugin_install.rs")),
    ] {
        for line in source.lines().filter(|l| !l.trim_start().starts_with("//")) {
            assert!(!line.contains(['\u{2014}', '\u{2013}']), "{file}: {line}");
        }
    }
}

/// File › Plugins… is one row that opens Preferences on the Plugins page, at any window width.
#[test]
fn file_plugins_row_opens_preferences_on_the_plugins_page() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let rows = crate::toolbar::file_rows(&app);
    let item = rows
        .iter()
        .find_map(|r| match r {
            crate::app_bar::FileRow::Item(i) if i.id == "plugins.preferences" => Some(i),
            _ => None,
        })
        .expect("File has a Plugins… row");
    assert_eq!(item.label, format!("{}…", app.tr("ui.plugins")));
    assert!(!rows.iter().any(|r| matches!(r, crate::app_bar::FileRow::Sub { label, .. } if label == app.tr("ui.plugins"))), "no Plugins submenu");
    crate::toolbar::run_file_row(&mut app, "plugins.preferences");
    assert!(app.prefs_window.shell.open);
}

/// Each plugin entry has its own dialog: opening a second one leaves the first open with its values.
#[test]
fn each_plugin_entry_has_its_own_dialog_with_its_own_values() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let with_size = |name: &str| {
        let mut e = entry(name, "edit", json!([]));
        e.input_schema = json!({"type":"object","properties":{"size":{"type":"number","default":1.0}}});
        e
    };
    app.plugin_menu.listed = vec![listed("gears", vec![with_size("spur")]), listed("bolts", vec![with_size("hex")])];
    open_entry(&mut app, 0, 0);
    app.plugin_menu.dialogs[0].fields.as_mut().unwrap()[0].1 = Input::Number(42.0);
    open_entry(&mut app, 1, 0);
    assert_eq!(app.plugin_menu.dialogs.iter().map(|d| d.entry.as_str()).collect::<Vec<_>>(), ["spur", "hex"], "both stay open");
    assert_ne!(app.plugin_menu.dialogs[0].id, app.plugin_menu.dialogs[1].id, "a window each");
    let size = |d: &EntryDialog| d.fields.as_ref().unwrap()[0].1.clone();
    assert!(matches!(size(&app.plugin_menu.dialogs[0]), Input::Number(v) if v == 42.0));
    assert!(matches!(size(&app.plugin_menu.dialogs[1]), Input::Number(v) if v != 42.0), "no value leaks to the other plugin");
    // Choosing an entry that is already open keeps its dialog as it is.
    open_entry(&mut app, 0, 0);
    assert_eq!(app.plugin_menu.dialogs.len(), 2);
    assert!(matches!(size(&app.plugin_menu.dialogs[0]), Input::Number(v) if v == 42.0));
    // Both draw, and closing one leaves the other.
    let mut h = crate::scenario::Harness::new();
    h.app.plugin_menu.listed = app.plugin_menu.listed.clone();
    open_entry(&mut h.app, 0, 0);
    open_entry(&mut h.app, 1, 0);
    h.frames(3);
    assert_eq!(h.app.plugin_menu.dialogs.len(), 2, "both are drawn and stay open");
    h.app.plugin_menu.dialogs.remove(0);
    h.frames(2);
    assert_eq!(h.app.plugin_menu.dialogs.iter().map(|d| d.entry.as_str()).collect::<Vec<_>>(), ["hex"]);
}

/// A reopened plugin command keeps its window position within the design. Another entry gets
/// a separate window, while the serial id of each new run still changes (#18684).
#[test]
fn plugin_dialog_reopen_remembers_dragged_window_position() {
    let mut h = crate::scenario::Harness::new();
    assert!(!h.app.preferences.plugin_remember_dialog_position);
    h.app.preferences.plugin_remember_dialog_position = true;
    h.app.plugin_menu.listed = vec![listed("gears", vec![entry("spur", "edit", json!([])), entry("hex", "edit", json!([]))])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    h.frames(4);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    let (first_serial, first_id) = (dialog.id, dialog_window_id(dialog, true));
    assert_ne!(dialog_window_id(dialog, false), first_id, "opt-out uses the old serial-based identity");
    let first = h.ctx.memory(|m| m.area_rect(first_id)).expect("window is placed");
    let header = crate::scenario::rect_of("dialog:header").unwrap();
    let from = egui::pos2(header.left() + 48.0, header.center().y);
    h.call("ui.drag", json!({"x0":from.x,"y0":from.y,"x1":from.x-105.0,"y1":from.y+55.0,"steps":8}));
    h.frames(5);
    let dragged = h.ctx.memory(|m| m.area_rect(first_id)).unwrap();
    assert!((dragged.min - first.min).length() > 30.0, "drag moved the window: {first:?} -> {dragged:?}");

    h.step(&json!({"click":{"handle":"plugin_dialog:cancel"}})).unwrap();
    h.frames(2);
    assert!(h.app.plugin_menu.dialogs.is_empty(), "Cancel closes the old instance");
    open_entry(&mut h.app, 0, 0);
    h.frames(4);
    let reopened = h.app.plugin_menu.dialog().unwrap();
    assert_ne!(reopened.id, first_serial, "a new run gets a new serial");
    assert_eq!(dialog_window_id(reopened, true), first_id, "the command's window keeps its egui identity");
    let again = h.ctx.memory(|m| m.area_rect(first_id)).unwrap();
    assert!((again.min - dragged.min).length() <= 2.0, "reopening keeps the moved position: {dragged:?} -> {again:?}");

    open_entry(&mut h.app, 0, 1);
    let other = h.app.plugin_menu.dialog().unwrap();
    assert_ne!(dialog_window_id(other, true), first_id, "a different entry has its own position");
}

/// A 20 x 20 x 10 box in the home view, with a plugin whose one command has [`pick_schema`].
fn box_and_pick_plugin() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[0,0,0]})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"home"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let mut seats = entry("seats", "edit", json!([]));
    seats.input_schema = pick_schema();
    h.app.plugin_menu.listed = vec![listed("inserts", vec![seats])];
    // The test's own list: the installed plugins are not read over it.
    h.app.plugin_menu.loaded = true;
    h
}

/// Open the plugin's dialog. Its window is in the middle of the view: the picks below are made
/// on the box's bottom front edge and its right face, beside it.
fn open_dialog(h: &mut crate::scenario::Harness) {
    open_entry(&mut h.app, 0, 0);
    h.frames(2);
}

fn picks(h: &crate::scenario::Harness) -> Vec<Vec<Sel>> {
    let dialog = h.app.plugin_menu.dialog().unwrap();
    dialog.fields.iter().flatten().filter_map(|(_, input)| if let Input::Selection(p) = input { Some(p.clone()) } else { None }).collect()
}

#[test]
fn selection_fields_pick_in_the_viewport_each_into_its_own_field() {
    let mut h = box_and_pick_plugin();
    open_dialog(&mut h);
    let id = h.app.plugin_menu.dialog().unwrap().id;
    assert_eq!(picking(&h.app), Some(id), "the first selection field takes the viewport's picks");
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    // An edge clicked in the view goes into Edges, and nowhere else.
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    h.frames(2);
    let [edges, body] = &picks(&h)[..] else { panic!() };
    assert!(matches!(edges[..], [Sel::Edge { .. }]), "{edges:?}");
    assert!(body.is_empty(), "the other field keeps its own picks");
    // Body to modify, clicked: the next click picks the body there.
    h.step(&json!({"click":{"handle":"plugin_dialog:field:body"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().map(|d| d.active), Some(1), "the clicked field is the active one");
    h.step(&json!({"click":{"world":[20,10,5]}})).unwrap();
    h.frames(2);
    let [edges, body] = &picks(&h)[..] else { panic!() };
    assert_eq!(edges.len(), 1, "picking the body leaves Edges alone");
    let Sel::Edge { body: name, .. } = &edges[0] else { panic!() };
    assert_eq!(body, &[Sel::Body { name: name.clone() }]);
    let sent = h.app.plugin_menu.dialog().unwrap().ready(&h.app).expect("both filled");
    assert_eq!(sent["edges"][0]["type"], "edge");
    assert_eq!(sent["body"][0]["type"], "body");
    // Cancel: the picking stops with the window, and the design is as it was.
    let doc = h.app.session.doc.clone();
    h.app.plugin_menu.dialogs.clear();
    h.frames(2);
    assert!(h.app.dialog.is_none(), "no picks are taken for a closed dialog");
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
}

#[test]
fn the_selection_fills_only_the_first_field_and_a_design_change_clears_the_picks() {
    let mut h = box_and_pick_plugin();
    // An edge and the body selected: Edges takes the edge, Body to modify stays empty.
    h.step(&json!({"click":{"world":[10,0,10]}})).unwrap();
    h.frames(2);
    let Some(Sel::Edge { body, .. }) = h.app.session.selection.first().cloned() else { panic!("{:?}", h.app.session.selection) };
    h.app.session.selection.push(Sel::Body { name: body.clone() });
    open_dialog(&mut h);
    let [edges, picked_body] = &picks(&h)[..] else { panic!() };
    assert!(matches!(edges[..], [Sel::Edge { .. }]), "{edges:?}");
    assert!(picked_body.is_empty(), "one selection never fills two fields");
    // The design changes under the open dialog: the picks are cleared, and it says so.
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[40,0,0]})).unwrap();
    h.frames(2);
    assert!(picks(&h).iter().all(Vec::is_empty), "references into the old model are dropped");
    assert!(h.app.plugin_menu.dialog().unwrap().picks_cleared);
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app), Err(solidcraft_ui_text::format("ui.plugin_dialog_missing", &[("field", "Edges")])));
    // Turning the view changes nothing.
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    h.frames(2);
    h.step(&json!({"call":"ui.view","params":{"view":"top"}})).unwrap();
    h.settle();
    assert_eq!(picks(&h)[0].len(), 1, "the camera is not the design");
    assert!(!h.app.plugin_menu.dialog().unwrap().picks_cleared);
}

#[test]
fn a_profile_field_keeps_profiles_from_several_sketches() {
    let mut h = crate::scenario::Harness::new();
    for x in [0, 40] {
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[x,0],"p1":[x + 20,20]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.app.run("select.clear", json!({})).unwrap();
    let mut make = entry("make", "edit", json!([]));
    make.input_schema =
        json!({"type":"object","properties":{"profiles":{"type":"array","x-solidcraft-selection":{"version":1,"accept":["profile"]}}}});
    h.app.plugin_menu.listed = vec![listed("shapes", vec![make])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    h.frames(2);
    let sketches: Vec<u64> =
        h.app.session.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. })).map(|f| f.id).collect();
    assert_eq!(sketches.len(), 2);
    for sketch in &sketches {
        let session = h.app.session.scratch();
        h.app.dialog.as_mut().unwrap().pick(&session, Sel::Profile { sketch: *sketch, index: 0 });
    }
    h.frames(2);
    assert_eq!(picks(&h)[0].len(), 2, "a plugin's profile field is not limited to one sketch: {:?}", picks(&h));
}

#[test]
fn an_open_tool_keeps_the_viewport_until_it_is_done() {
    let mut h = box_and_pick_plugin();
    h.step(&json!({"start":"solid.fillet"})).unwrap();
    h.frames(2);
    open_dialog(&mut h);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Fillet { .. })), "Fillet keeps picking");
    h.step(&json!({"click":{"handle":"plugin_dialog:field:edges"}})).unwrap();
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Fillet { .. })), "the click asks; it does not cancel Fillet");
    assert_eq!(h.app.status.as_ref().map(|(text, _, error)| (text.as_str(), *error)), Some((h.app.tr("ui.plugin_dialog_finish_tool"), true)));
    // Fillet is done: the field picks once clicked.
    h.app.dialog = None;
    h.frames(2);
    assert_eq!(picking(&h.app), None, "nothing takes the viewport by itself");
    h.step(&json!({"click":{"handle":"plugin_dialog:field:edges"}})).unwrap();
    h.frames(2);
    assert_eq!(picking(&h.app), Some(h.app.plugin_menu.dialog().unwrap().id), "with Fillet closed the field picks");
    // Escape cancels the plugin's dialog as it does a native tool's, and the picking stops with it.
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_none());
    assert!(h.app.plugin_menu.dialogs.is_empty());
}

#[test]
fn ungranted_command_groups_keep_run_off_and_are_named() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let mut plugin = listed("files", vec![entry("export", "edit", json!([]))]);
    plugin.groups = ["file".to_string()].into();
    app.plugin_menu.listed = vec![plugin];
    open_entry(&mut app, 0, 0);
    let dialog = app.plugin_menu.dialog().unwrap();
    let why = dialog.ready(&app).unwrap_err();
    assert!(why.contains("file"), "{why}");
    assert!(run(&mut app).is_err() && app.control_held.is_empty(), "nothing is queued");
    app.session.plugin_state.command_groups.insert("file".into());
    assert!(app.plugin_menu.dialog().unwrap().ready(&app).is_err(), "legacy global grants do not approve this plugin");
    app.session.plugin_state.grants.entry("other".into()).or_default().command_groups.insert("file".into());
    assert!(app.plugin_menu.dialog().unwrap().ready(&app).is_err(), "another plugin's grants do not approve this plugin");
    app.session.plugin_state.grants.entry("files".into()).or_default().command_groups.insert("file".into());
    assert!(app.plugin_menu.dialog().unwrap().ready(&app).is_ok());
}

#[test]
fn a_previewed_run_has_none_of_the_plugins_grants() {
    let mut session = solidcraft_engine::Session::default();
    let grants = session.plugin_state.grants.entry("inserts".into()).or_default();
    grants.package_hash = Some("hash".into());
    grants.command_groups.insert("file".into());
    grants.folders.push(serde_json::from_value(json!({"name":"out","folder":"/tmp/out","read":true,"write":true})).unwrap());
    grants.input_folder = true;
    session.plugin_state.grants.entry("other".into()).or_default().command_groups.insert("doc".into());
    dry_run(&mut session, "inserts");
    let grants = &session.plugin_state.grants["inserts"];
    assert!(grants.folders.is_empty() && grants.command_groups.is_empty() && !grants.input_folder, "no files, no groups");
    assert_eq!(grants.package_hash.as_deref(), Some("hash"), "still the approved package");
    assert!(session.plugin_state.file_grants.is_empty());
    assert!(session.plugin_state.grants["other"].command_groups.contains("doc"), "other plugins keep theirs");
}

#[cfg(all(not(target_arch = "wasm32"), any(target_arch = "x86_64", target_arch = "aarch64")))]
mod native {
    use super::*;

    /// A package whose entry `make` passes its parameters to `solid.box`.
    fn package(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("solidcraft-plugin-menu-{}-{tag}", std::process::id()));
        let dir = root.join("boxes");
        std::fs::create_dir_all(&dir).unwrap();
        let (name, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
        let wat = include_str!("plugin_menu_fixture.wat").replace("PLUGIN_PACKAGE", name).replace("PLUGIN_VERSION", version);
        std::fs::write(dir.join("plugin.wasm"), wat::parse_str(&wat).unwrap()).unwrap();
        let schema = json!({"type":"object","required":["length","width","height"],"properties":{
            "length":{"type":"number","title":"Length","unit":"mm","exclusiveMinimum":0,"default":20},
            "width":{"type":"number","title":"Width","unit":"mm","exclusiveMinimum":0,"default":10},
            "height":{"type":"number","title":"Height","unit":"mm","exclusiveMinimum":0,"default":3}}});
        let manifest = json!({"id":"boxes","name":"Boxes","version":"0.1.0","capabilities":["commands@0.1"],"runtime":{
            "module":"plugin.wasm","entries":[{"name":"make","effect":"edit","input_schema":schema,"slots":[{"kind":"menu","name":"Box"}]}],
            "commands":["solid.box"]}});
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        root
    }

    fn settle(h: &mut crate::scenario::Harness) {
        for _ in 0..500 {
            h.frames(1);
            if h.app.plugin_menu.dialog().is_none_or(|d| d.running.is_none()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the run did not finish");
    }

    #[test]
    fn running_the_dialog_matches_plugin_run_with_the_same_params_as_one_undo_step() {
        let root = package("same");
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        assert_eq!(h.app.plugin_menu.listed.len(), 1);
        let plugin = h.app.plugin_menu.listed[0].clone();
        // The same parameters through plugin.run, on a copy of the same design.
        let mut reference = h.app.session.scratch();
        let inspected = reference.execute("plugin.inspect", &json!({"path":plugin.path})).unwrap();
        assert_eq!(inspected["approval"], plugin.approval, "the menu's hash is the reviewed package hash");
        let params = json!({"length":30.0,"width":10.0,"height":3.0});
        reference.execute("plugin.run", &json!({"path":plugin.path,"approval":plugin.approval,"entry":"make","params":params})).unwrap();

        open_entry(&mut h.app, 0, 0);
        let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
        assert_eq!(dialog.label, "Box");
        let fields = dialog.fields.as_mut().unwrap();
        fields.iter_mut().find(|(f, _)| f.key == "length").unwrap().1 = Input::Number(30.0);
        assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app), Ok(params));
        let undo = h.app.session.undo.len();
        let before = h.app.session.doc.clone();
        run(&mut h.app).unwrap();
        settle(&mut h);
        assert!(h.app.plugin_menu.dialogs.is_empty(), "an edit closes the dialog");
        assert_eq!(h.app.session.doc.as_ref(), reference.doc.as_ref(), "same document as plugin.run");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.as_ref(), before.as_ref(), "undo takes back the whole run");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_run_is_for_the_design_its_dialog_opened_on() {
        let root = package("tabs");
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        open_entry(&mut h.app, 0, 0);
        let first = h.app.session.uid;
        // Another tab, then Run: refused at once, nothing queued.
        crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
        assert_ne!(h.app.session.uid, first);
        assert_eq!(run(&mut h.app), Err(h.app.tr("ui.plugin_dialog_other_design").to_string()));
        assert!(h.app.control_held.is_empty());
        // Back on its design it runs; a switch while the request waits in the queue refuses it.
        crate::documents::switch(&mut h.app, 0);
        assert_eq!(h.app.session.uid, first);
        run(&mut h.app).unwrap();
        crate::documents::switch(&mut h.app, 1);
        let other = h.app.session.doc.clone();
        settle(&mut h);
        let error = h.app.plugin_menu.dialog().and_then(|d| d.error.clone()).expect("refused in the dialog");
        assert!(error.contains(h.app.tr("ui.plugin_dialog_other_design")), "{error}");
        assert!(std::sync::Arc::ptr_eq(&other, &h.app.session.doc), "the other design is untouched");
        crate::documents::switch(&mut h.app, 0);
        assert!(h.app.session.undo.is_empty(), "nor is its own");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_explicitly_inspected_package_wins_over_an_installed_one_with_its_id() {
        let installed = package("installed");
        let inspected = package("inspected");
        let mut h = crate::scenario::Harness::new();
        let path = inspected.join("boxes").to_string_lossy().into_owned();
        h.app.session.plugin_state.packages.insert("boxes".into(), path.clone());
        let listed = list(&h.app, Some(&installed));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, path);
        let _ = std::fs::remove_dir_all(installed);
        let _ = std::fs::remove_dir_all(inspected);
    }

    #[test]
    fn a_failed_run_says_why_in_the_dialog_and_changes_nothing() {
        let root = package("fails");
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        open_entry(&mut h.app, 0, 0);
        // The package changed after the dialog read it: its hash no longer matches.
        std::fs::write(root.join("boxes/plugin.json"), std::fs::read_to_string(root.join("boxes/plugin.json")).unwrap() + " ").unwrap();
        let doc = h.app.session.doc.clone();
        h.app.home.open = true;
        run(&mut h.app).unwrap();
        settle(&mut h);
        let error = h.app.plugin_menu.dialog().and_then(|d| d.error.clone()).expect("the dialog stays with its error");
        assert!(error.contains("approval"), "{error}");
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert!(h.app.home.open, "a failed run leaves the Start page up");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_edit_from_the_start_page_closes_it_and_lands_in_the_design() {
        let root = package("start");
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        open_entry(&mut h.app, 0, 0);
        let before = h.app.session.doc.clone();
        assert!(before.features.is_empty(), "a blank design");
        run(&mut h.app).unwrap();
        settle(&mut h);
        assert!(!h.app.home.open, "the Start page closes when the design changes");
        assert!(!std::sync::Arc::ptr_eq(&before, &h.app.session.doc), "the edit replaced the document");
        assert!(!h.app.session.doc.features.is_empty(), "the edit is in the design");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_plugins_menu_opens_from_the_ribbon_group_and_its_entry_opens_the_dialog() {
        let root = package("menu");
        let mut h = crate::scenario::Harness::new();
        h.app.session.plugin_state.packages.insert("boxes".into(), root.join("boxes").to_string_lossy().into_owned());
        h.frames(2);
        // The ribbon's Plugins group label opens the menu below it.
        let ctx = h.ctx.clone();
        toggle_below(&mut h.app, &ctx, egui::Rect::from_min_size(egui::pos2(300.0, 100.0), egui::vec2(60.0, 20.0)));
        h.frames(2);
        assert!(h.app.plugin_menu.open && h.app.plugin_menu.listed.len() == 1);
        h.step(&json!({"widget":"Box…"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.plugin_menu.dialog().map(|d| d.entry.as_str()), Some("make"));
        assert!(crate::scenario::handle_at("plugin_dialog:field:length").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    /// Frames until the preview is computed.
    fn previewed(h: &mut crate::scenario::Harness) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        h.frames(2);
        while h.app.preview.busy {
            assert!(std::time::Instant::now() < deadline, "the preview did not finish");
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.frames(1);
        }
        h.frames(1);
    }

    #[test]
    fn the_host_previews_what_run_would_change_and_changes_nothing() {
        let root = package("preview");
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        h.app.plugin_menu.loaded = true;
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        open_entry(&mut h.app, 0, 0);
        previewed(&mut h);
        assert!(h.app.preview.active, "the box the entry makes is drawn: {:?}", h.app.preview.error);
        let size = |h: &crate::scenario::Harness| {
            let b = h.app.preview.bounds;
            ((b.max.x - b.min.x).round(), (b.max.y - b.min.y).round(), (b.max.z - b.min.z).round())
        };
        assert_eq!(size(&h), (20.0, 10.0, 3.0), "the defaults");
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc), "the design is untouched");
        assert_eq!(h.app.session.undo.len(), undo, "no undo step");
        // A changed value is previewed again.
        h.app.plugin_menu.dialogs[0].fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "length").unwrap().1 = Input::Number(30.0);
        previewed(&mut h);
        assert_eq!(size(&h), (30.0, 10.0, 3.0));
        // Preview off: nothing is drawn. A read entry or a native tool is never previewed here.
        h.app.plugin_menu.dialogs[0].preview = false;
        previewed(&mut h);
        assert!(!h.app.preview.active);
        h.app.plugin_menu.dialogs[0].preview = true;
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        assert!(preview_commands(&h.app).is_none(), "the open tool's preview is the one shown");
        h.app.dialog = None;
        // Run applies the same box, once.
        previewed(&mut h);
        run(&mut h.app).unwrap();
        settle(&mut h);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(h.app.plugin_menu.dialogs.is_empty());
        h.frames(2);
        assert!(!h.app.preview.active, "no dialog, no preview");
        let _ = std::fs::remove_dir_all(root);
    }

    /// A package with `home` (a box, then view.home), `edit` (a box and no view command), `fail`
    /// and `place` (a component with a box in it, moved 100 mm along X).
    fn view_package(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("solidcraft-plugin-view-{}-{tag}", std::process::id()));
        let dir = root.join("frames");
        std::fs::create_dir_all(&dir).unwrap();
        let (name, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
        let wat = include_str!("plugin_view_fixture.wat").replace("PLUGIN_PACKAGE", name).replace("PLUGIN_VERSION", version);
        std::fs::write(dir.join("plugin.wasm"), wat::parse_str(&wat).unwrap()).unwrap();
        let entry = |entry_name: &str, label: &str| json!({"name": entry_name, "effect": "edit", "input_schema": {"type": "object"}, "slots": [{"kind": "menu", "name": label}]});
        let manifest = json!({"id":"frames","name":"Frames","version":"0.1.0","capabilities":["commands@0.1"],"runtime":{
            "module":"plugin.wasm",
            "entries":[entry("home", "Home"), entry("edit", "Edit"), entry("fail", "Fail"), entry("place", "Place")],
            "commands":["solid.box", "view.home", "component.create", "occurrence.move"]}});
        std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
        root
    }

    /// A run that makes a component and moves it is previewed where the run leaves it, also in
    /// a design where nothing was placed before: the preview used to show it on the origin
    /// then, and where it goes only once the design had a moved component.
    #[test]
    fn a_component_a_run_makes_and_moves_is_previewed_where_it_goes() {
        let root = view_package("place");
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        h.app.plugin_menu.loaded = true;
        assert!(!solidcraft_engine::frames::placed(&h.app.session), "a design with nothing placed");
        open_entry(&mut h.app, 0, 3);
        previewed(&mut h);
        assert!(h.app.preview.active, "the moved box is drawn: {:?}", h.app.preview.error);
        let b = h.app.preview.bounds;
        assert_eq!((b.min.x.round(), b.max.x.round()), (100.0, 110.0), "where the run moved it, not on the origin");
        // The triangles the preview draws are the box there, not only the bounds it reports.
        let (_, id, cmds) = preview_commands(&h.app).expect("the dialog is previewed");
        let mut scratch = h.app.session.scratch();
        dry_run(&mut scratch, &id);
        let colors = crate::preview::Colors::from_tokens(&solidcraft_theme::Tokens::get());
        let built = crate::preview::compute(&scratch, &cmds, colors).unwrap();
        let (vertices, rest) = built.scene.tris.as_chunks::<{ crate::gpu::TRI_SIZE }>();
        assert!(rest.is_empty() && !vertices.is_empty(), "the box is drawn");
        let xs = vertices.iter().map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]));
        let (lo, hi) = xs.fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        assert_eq!((lo.round(), hi.round()), (100.0, 110.0), "drawn where the run moved it");
        // Run leaves it there too.
        run(&mut h.app).unwrap();
        settle(&mut h);
        use solidcraft_engine::geom::MeshGeom as _;
        let world = h.app.session.world_state().unwrap();
        let placed = world.bodies.iter().fold(solidcraft_engine::geom::Aabb3::EMPTY, |acc, body| acc.union(&body.mesh().bounds()));
        assert_eq!((placed.min.x.round(), placed.max.x.round()), (100.0, 110.0));
        let _ = std::fs::remove_dir_all(root);
    }

    fn same_camera(a: &solidcraft_viewport::Camera, b: &solidcraft_viewport::Camera) -> bool {
        let near =
            |p: solidcore_record::Vec3, q: solidcore_record::Vec3| (p.x - q.x).abs() < 1e-6 && (p.y - q.y).abs() < 1e-6 && (p.z - q.z).abs() < 1e-6;
        near(a.target, b.target) && (a.distance - b.distance).abs() < 1e-6 && (a.yaw - b.yaw).abs() < 1e-9 && (a.pitch - b.pitch).abs() < 1e-9
    }

    #[test]
    fn a_view_command_from_a_plugin_moves_the_camera_and_an_edit_without_one_leaves_it() {
        let root = view_package("camera");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        assert_eq!(h.app.plugin_menu.listed.len(), 1);
        let index = |name: &str| h.app.plugin_menu.listed[0].entries.iter().position(|entry| entry.name == name).expect(name);
        let home = index("home");
        let _ = (index("edit"), index("fail"));
        h.app.cam.set_view(solidcraft_viewport::StandardView::Front);
        h.app.cam.distance = 800.0;
        h.app.cam.target = solidcore_record::Vec3::ZERO;
        let parked = h.app.cam;

        open_entry(&mut h.app, 0, home);
        run(&mut h.app).unwrap();
        settle(&mut h);
        h.frames(2);
        assert_eq!(h.app.session.doc.features.len(), 1, "the entry inserted its box");
        let from_plugin = h.app.cam;
        h.app.cancel_view_animation();
        h.app.cam = parked;
        h.app.run("view.home", json!({})).unwrap();
        h.frames(2);
        assert!(same_camera(&from_plugin, &h.app.cam), "view.home frames the inserted box the way Home does: {from_plugin:?} vs {:?}", h.app.cam);
        let centre = h.app.cam.target;
        let on_box = (centre.x - 5.0).abs() < 5.0 && (centre.y - 5.0).abs() < 5.0 && (centre.z - 5.0).abs() < 5.0;
        assert!(on_box, "the frame is on that box: {centre:?}");

        let stayed = h.app.cam;
        // A frame can open the Plugins menu, and that reads the installed plugins again.
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let edit = h.app.plugin_menu.listed[0].entries.iter().position(|entry| entry.name == "edit").expect("edit");
        open_entry(&mut h.app, 0, edit);
        let opened = h.app.plugin_menu.dialog().map(|d| d.entry.clone());
        assert_eq!(
            opened.as_deref(),
            Some("edit"),
            "the second entry's dialog opened; listed {:?}",
            h.app.plugin_menu.listed.iter().map(|p| (p.id.clone(), p.entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>())).collect::<Vec<_>>()
        );
        run(&mut h.app).unwrap();
        assert!(h.app.plugin_menu.dialog().is_some_and(|d| d.running.is_some()), "the second entry was queued");
        settle(&mut h);
        h.frames(2);
        let why = h
            .app
            .plugin_menu
            .dialog()
            .map(|d| d.error.clone().unwrap_or_else(|| format!("dialog still open for {}", d.entry)))
            .unwrap_or_else(|| "dialog closed".into());
        let names: Vec<_> = h.app.session.doc.features.iter().map(|f| f.name.clone()).collect();
        assert_eq!(h.app.session.doc.features.len(), 2, "the second entry edited the design: {why}; features {names:?}; status {:?}", h.app.status);
        assert!(same_camera(&h.app.cam, &stayed), "an edit with no view command leaves the camera: {:?} vs {stayed:?}", h.app.cam);

        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let fail = h.app.plugin_menu.listed[0].entries.iter().position(|entry| entry.name == "fail").expect("fail");
        open_entry(&mut h.app, 0, fail);
        run(&mut h.app).unwrap();
        settle(&mut h);
        h.frames(2);
        let error = h.app.plugin_menu.dialog().and_then(|d| d.error.clone()).expect("the failed run stays in the dialog");
        assert!(!error.is_empty(), "{error}");
        assert_eq!(h.app.session.doc.features.len(), 2, "the failed run changes nothing");
        assert!(same_camera(&h.app.cam, &stayed), "a failed run does not move the camera: {:?} vs {stayed:?}", h.app.cam);
        let _ = std::fs::remove_dir_all(root);
    }
}
