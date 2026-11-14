//! The Plugins menu and its entry dialog: schema fields, required gating, and a run through the
//! dialog matching `plugin.run`.
use super::*;

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
    assert_eq!(fields[5].0.kind, Kind::Choice(vec!["14.5".into(), "20".into(), "25".into()]));
    assert_eq!(fields[5].1, Input::Choice(Some(1)), "the default");
    let no_default = form(&json!({"type":"object","properties":{"p":{"type":"string","enum":["a","b"]}}})).unwrap();
    assert_eq!(no_default[0].1, Input::Choice(None), "no default: nothing chosen yet");
    assert_eq!(form(&json!({"type":"object"})), Some(Vec::new()), "no properties: nothing to fill");
}

#[test]
fn selections_targets_and_nested_objects_are_not_guessed() {
    for property in [
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
        approval: String::new(),
        groups: BTreeSet::new(),
        entries,
    }
}

#[test]
fn the_menu_lists_install_then_entries_with_a_submenu_for_several() {
    use crate::app_bar::FileRow;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let empty = rows(&app);
    assert!(matches!(&empty[0], FileRow::Item(i) if i.id == INSTALL));
    assert!(matches!(&empty[1], FileRow::Separator));
    assert!(matches!(&empty[2], FileRow::Item(i) if i.label == app.tr("ui.plugin_menu_empty") && !i.enabled));
    app.plugin_menu.listed = vec![
        listed("gears", vec![entry("spur", "edit", json!([{"kind":"menu","name":"Spur Gear"}]))]),
        listed("tools", vec![entry("measure", "read", json!([])), entry("tidy", "edit", json!([{"kind":"ribbon","name":"x"}]))]),
    ];
    let listed_rows = rows(&app);
    assert_eq!(listed_rows.len(), 4);
    assert!(matches!(&listed_rows[2], FileRow::Item(i) if i.id == "entry:0:0" && i.label == "Spur Gear…"), "menu slot names the row");
    let FileRow::Sub { label, rows: sub } = &listed_rows[3] else { panic!("several entries: a submenu") };
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
    assert!(app.plugin_menu.dialog.is_none(), "listing opens nothing");

    let menu = rows(&app);
    assert!(matches!(&menu[2], FileRow::Item(i) if i.id == "entry:0:0" && i.label == "gears plugin…"));
    assert!(matches!(&menu[3], FileRow::Sub { label, rows } if label == "tools plugin" && rows.len() == 2));

    // The ribbon button opens that one command's dialog, titled by it alone.
    assert!(choose_ribbon(&mut app, "plugin_entry:tools:tidy"));
    let dialog = app.plugin_menu.dialog.take().expect("its dialog");
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
    assert_eq!(menu.len(), 3, "off: no menu row");
    assert!(matches!(&menu[2], FileRow::Sub { label, .. } if label == "tools plugin"));
    choose_ribbon(&mut app, "plugin_entry:gears:spur");
    choose(&mut app, "entry:0:0");
    assert!(app.plugin_menu.dialog.is_none(), "a plugin turned off opens nothing");

    app.preferences.disabled_plugins.insert("tools".into());
    assert!(ribbon_buttons(&app).is_empty());
    assert!(matches!(&rows(&app)[2], FileRow::Item(i) if i.label == app.tr("ui.plugin_menu_all_off") && !i.enabled));
}

#[test]
fn plugin_dialog_titles_are_the_command_alone_and_plugin_text_has_no_long_dashes() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let mut gears = listed("gears", vec![entry("spur", "edit", json!([]))]);
    gears.name = "Gears".into();
    app.plugin_menu.listed = vec![gears];
    open_entry(&mut app, 0, 0);
    assert_eq!(app.plugin_menu.dialog.as_ref().map(|d| d.label.as_str()), Some("Gears"), "no \"- Plugins\" after it");
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

/// A narrow app bar puts the same rows in the File menu, under `plugins:`, and they act the same.
#[test]
fn the_file_submenu_holds_the_same_rows_and_opens_the_same_dialog() {
    use crate::app_bar::FileRow;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.plugin_menu.listed = vec![listed("gears", vec![entry("spur", "edit", json!([]))])];
    let FileRow::Sub { label, rows } = file_submenu(&app) else { panic!("a submenu") };
    assert_eq!(label, app.tr("ui.plugins"));
    let ids: Vec<&str> = rows.iter().filter_map(|r| if let FileRow::Item(i) = r { Some(i.id.as_str()) } else { None }).collect();
    assert_eq!(ids, ["plugins:install", "plugins:entry:0:0"]);
    choose(&mut app, "entry:0:0");
    assert_eq!(app.plugin_menu.dialog.as_ref().map(|d| d.entry.as_str()), Some("spur"));
}

#[test]
fn ungranted_command_groups_keep_run_off_and_are_named() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let mut plugin = listed("files", vec![entry("export", "edit", json!([]))]);
    plugin.groups = ["file".to_string()].into();
    app.plugin_menu.listed = vec![plugin];
    open_entry(&mut app, 0, 0);
    let dialog = app.plugin_menu.dialog.as_ref().unwrap();
    let why = dialog.ready(&app).unwrap_err();
    assert!(why.contains("file"), "{why}");
    assert!(run(&mut app).is_err() && app.control_held.is_empty(), "nothing is queued");
    app.session.plugin_state.command_groups.insert("file".into());
    assert!(app.plugin_menu.dialog.as_ref().unwrap().ready(&app).is_ok());
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
            if h.app.plugin_menu.dialog.as_ref().is_none_or(|d| d.running.is_none()) {
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
        let dialog = h.app.plugin_menu.dialog.as_mut().unwrap();
        assert_eq!(dialog.label, "Box");
        let fields = dialog.fields.as_mut().unwrap();
        fields.iter_mut().find(|(f, _)| f.key == "length").unwrap().1 = Input::Number(30.0);
        assert_eq!(h.app.plugin_menu.dialog.as_ref().unwrap().ready(&h.app), Ok(params));
        let undo = h.app.session.undo.len();
        let before = h.app.session.doc.clone();
        run(&mut h.app).unwrap();
        settle(&mut h);
        assert!(h.app.plugin_menu.dialog.is_none(), "an edit closes the dialog");
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
        let error = h.app.plugin_menu.dialog.as_ref().and_then(|d| d.error.clone()).expect("refused in the dialog");
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
        run(&mut h.app).unwrap();
        settle(&mut h);
        let error = h.app.plugin_menu.dialog.as_ref().and_then(|d| d.error.clone()).expect("the dialog stays with its error");
        assert!(error.contains("approval"), "{error}");
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_plugins_menu_opens_from_the_app_bar_and_its_entry_opens_the_dialog() {
        let root = package("menu");
        let mut h = crate::scenario::Harness::new();
        h.app.session.plugin_state.packages.insert("boxes".into(), root.join("boxes").to_string_lossy().into_owned());
        h.frames(2);
        h.step(&json!({"click":{"handle":"appbar:plugins"}})).unwrap();
        h.frames(1);
        assert!(h.app.plugin_menu.open && h.app.plugin_menu.listed.len() == 1);
        h.step(&json!({"widget":"Box…"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.plugin_menu.dialog.as_ref().map(|d| d.entry.as_str()), Some("make"));
        assert!(crate::scenario::handle_at("plugin_dialog:field:length").is_some());
        let _ = std::fs::remove_dir_all(root);
    }
}
