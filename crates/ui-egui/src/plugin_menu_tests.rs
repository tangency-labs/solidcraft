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
        disabled: false,
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
