use super::*;
use serde_json::{Value, json};

#[test]
fn edit_form_prefill_validates_fields_and_programmatic_open_is_refused() {
    let mut s = Session::default();
    let schema = json!({"type":"object","required":["depth","edges"],"properties":{
        "depth":{"type":"number","minimum":0,"maximum":20},
        "edges":{"type":"array","x-solidcraft-selection":{"version":1,"accept":["edge"]}}
    }});
    assert!(form_errors(&s, &schema, &json!({"depth":5,"edges":[]})).is_empty());
    let errors = form_errors(&s, &schema, &json!({"depth":-1,"edges":[{"type":"edge","body":"gone","index":0,"point":[0,0,0]}]}));
    assert!(errors.contains_key("depth") && errors.contains_key("edges"), "{errors:?}");
    let face_schema = json!({"type":"object","properties":{"face":{"type":"array","x-solidcraft-selection":{"version":1,"accept":["face"]}}}});
    assert!(matches!(
        form_errors(&s, &face_schema, &json!({"face":[{"type":"face","body":"gone","index":0,"point":[0,0,0]}]})).get("face"),
        Some(crate::schema::plugin::FormFieldError::MissingPick)
    ));

    let before = s.doc.clone();
    let error = s.execute("plugin.edit", &json!({"plugin":"sample","entry":"make","params":{}})).unwrap_err();
    assert!(matches!(error.host_error(), Some(HostError::InteractiveRequired { .. })), "{error}");
    assert!(error.to_string().contains("call the entry with params"));
    assert_eq!(
        structured_tool_error(&error.to_string()),
        Some(json!({"kind":"interactive_required","entry":"sample.make","action":"call_entry_with_params"}))
    );
    let scripted = s.run_script(&json!([{"command":"plugin.edit","params":{"plugin":"sample","entry":"make","params":{}}}])).unwrap_err();
    assert!(scripted.to_string().contains("call the entry with params"), "{scripted}");
    assert_eq!(structured_tool_error(&scripted.to_string()), structured_tool_error(&error.to_string()));
    assert_eq!(s.doc, before);
}

#[test]
fn edit_form_prefill_uses_visible_controllers_and_reports_condition_errors() {
    let s = Session::default();
    let mut schema = json!({"type":"object","required":["secret"],"properties":{
        "gate":{"type":"boolean"},
        "mode":{"type":"boolean","x-solidcraft-visible-when":{"field":"gate","eq":true}},
        "secret":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}}
    }});
    let before = s.doc.clone();
    assert!(form_errors(&s, &schema, &json!({"gate":false,"mode":true,"secret":"hidden"})).is_empty());
    assert!(form_errors(&s, &schema, &json!({"gate":false,"mode":true})).is_empty());
    assert!(form_errors(&s, &schema, &json!({"gate":true,"mode":true,"secret":"invalid"})).contains_key("secret"));
    assert!(form_errors(&s, &schema, &json!({"gate":true,"mode":true})).contains_key("secret"));
    schema["properties"]["mode"]["x-solidcraft-visible-when"] = json!({"field":"missing","eq":true});
    let errors = form_errors(&s, &schema, &json!({"gate":false,"mode":true}));
    assert!(matches!(errors.get("params"), Some(crate::schema::plugin::FormFieldError::InvalidValue { .. })));
    assert_eq!(s.doc, before, "validating visibility never changes the design");
}

#[test]
fn edit_form_accepts_approximate_pick_coordinates_without_substituting_indices() {
    use solidcore_record::Vec3;
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius":10,"height":20,"body_name":"Stock"})).unwrap();
    let state = s.model.state();
    let body = state.body("Stock").unwrap();
    let wall = body.body.nearest_face(Vec3::new(10.0, 0.0, 10.0)).unwrap().1;
    let cap = body.body.nearest_face(Vec3::new(0.0, 0.0, 20.0)).unwrap().1;
    let bottom = crate::kernel::blend_edges(&body.body, &[Vec3::new(10.0, 0.0, 0.0)]).unwrap()[0];
    let top = crate::kernel::blend_edges(&body.body, &[Vec3::new(10.0, 0.0, 20.0)]).unwrap()[0];
    let schema = json!({"type":"object","properties":{
        "faces":{"type":"array","items":{"type":"object"},"x-solidcraft-selection":{"version":1,"accept":["face"]}},
        "edges":{"type":"array","items":{"type":"object"},"x-solidcraft-selection":{"version":1,"accept":["edge"]}}
    }});
    // A 1 micrometre chord inset is larger than the modelling tolerance but smaller than
    // the shared pick coordinate allowance (0.1% of this 20 x 20 x 20 mm body's exact bounds).
    let mut values = json!({
        "faces":[{"type":"face","body":"Stock","index":wall,"point":[9.999,0,10]}],
        "edges":[{"type":"edge","body":"Stock","index":bottom,"point":[9.999,0,0]}]
    });
    let before = s.doc.clone();
    assert!(form_errors(&s, &schema, &values).is_empty(), "normal picks can be approximate");
    values["faces"][0]["index"] = json!(cap);
    values["edges"][0]["index"] = json!(top);
    let errors = form_errors(&s, &schema, &values);
    assert!(matches!(errors.get("faces"), Some(crate::schema::plugin::FormFieldError::MissingPick)));
    assert!(matches!(errors.get("edges"), Some(crate::schema::plugin::FormFieldError::MissingPick)));
    assert_eq!(s.doc, before, "validation never substitutes a nearby entity or changes the design");
}

#[test]
fn deferred_forms_are_scoped_to_interactive_plugin_entries_and_rollback() {
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: vec![],
        view: None,
        groups: BTreeSet::new(),
        file_folders: vec![],
        file_read_folders: vec![],
        evaluation: None,
        forms: vec![],
        form_source: Some(("sample".into(), parse_manifest(&serde_json::to_vec(&manifest()).unwrap(), "sample").unwrap().runtime.unwrap().entries)),
    };
    assert!(matches!(host.open_form("make", &json!({})), Err(HostError::InteractiveRequired { .. })));
    host.session.plugin_forms_interactive = true;
    assert!(host.open_form("unknown", &json!({})).is_err());
    host.begin_transaction().unwrap();
    host.open_form("make", &json!({"depth":-10,"edges":["missing"]})).unwrap();
    assert_eq!(host.forms.len(), 1, "invalid prefill opens a form, not a failed run");
    assert_eq!(host.forms[0].plugin, "sample");
    host.rollback_transaction().unwrap();
    assert!(host.forms.is_empty());
}

#[test]
fn named_tool_refusals_keep_plugin_file_and_budget_details() {
    let session = Session::default();
    let mut m = manifest();
    m["runtime"]["permissions"] = json!({"files":[{"name":"project","write":true}]});
    let manifest = parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").unwrap();
    for file in [None, Some("part.smp")] {
        for (error, structured) in [
            (
                HostError::PermissionDenied { permission: "files.write:project".into() },
                json!({"plugin":"sample","grant":"files.write:project","approve":tools::APPROVE}),
            ),
            (
                HostError::PermissionDenied { permission: format!("{}write-file", craft_plugin::READ_ENTRY_IMPORT) },
                json!({"plugin":"sample","entry":"make","refused_import":"write-file"}),
            ),
            (
                HostError::OverCeiling { limit: "memory_mib".into(), requested: 2048, allowed: 128 },
                json!({"plugin":"sample","grant":"memory_mib","approve":tools::APPROVE}),
            ),
            (
                HostError::OverCeiling { limit: "fuel".into(), requested: MAX_APPROVED_FUEL, allowed: 100_000_000_000 },
                json!({"plugin":"sample","grant":"fuel","approve":tools::APPROVE}),
            ),
        ] {
            let failure = PluginFailure { plugin: "Sample".into(), file: file.map(String::from), exporting: false, error };
            let original = failure.text();
            let explained = explain_named(&session, &manifest, "make", EngineError::PluginFailed(Box::new(failure))).to_string();
            assert!(explained.starts_with(&original), "{explained}");
            assert_eq!(structured_tool_error(&explained), Some(structured), "{explained}");
        }
        for error in [
            HostError::Exhausted { limit: "fuel".into(), allowed: 100000 },
            HostError::OverCeiling { limit: "fuel".into(), requested: MAX_APPROVED_FUEL + 1, allowed: 100_000_000_000 },
        ] {
            let unchanged = explain_named(&session, &manifest, "make", error.clone().into());
            assert!(matches!(&unchanged, EngineError::Plugin(actual) if actual == &error));
            assert!(structured_tool_error(&unchanged.to_string()).is_none());
            let failure = PluginFailure { plugin: "Sample".into(), file: file.map(String::from), exporting: false, error };
            let original = failure.text();
            let unchanged = explain_named(&session, &manifest, "make", EngineError::PluginFailed(Box::new(failure)));
            assert!(matches!(unchanged, EngineError::PluginFailed(_)));
            assert_eq!(unchanged.to_string(), original);
            assert!(structured_tool_error(&unchanged.to_string()).is_none());
        }
    }
    assert!(matches!(explain_named(&session, &manifest, "make", HostError::Cancelled.into()), EngineError::Plugin(HostError::Cancelled)));
}

fn manifest() -> Value {
    json!({"id":"sample", "name":"Sample", "version":"0.1.0", "capabilities":["commands@0.1"],"runtime":{
        "module":"plugin.wasm","entries":["make"],"commands":["solid.box"],
        "limits":{"fuel":100000,"memory_mib":1,"deadline_ms":5000}}})
}
#[test]
fn manifest_is_inert_bounded_and_refuses_unknown_capabilities() {
    assert!(parse_manifest(br#"{"id":"sample","name":"Sample","version":"1"}"#, "sample").unwrap().runtime.is_none());
    assert!(parse_manifest(&serde_json::to_vec(&manifest()).unwrap(), "sample").is_ok());
    let mut explicit = manifest();
    explicit["runtime"]["limits"]["fuel"] = json!(100_000_000_000_u64);
    assert!(parse_manifest(&serde_json::to_vec(&explicit).unwrap(), "sample").is_ok());
    assert_eq!(Limits::default().fuel, 10_000_000);
    let old = parse_manifest(br#"{"id":"sample","name":" Sample ","version":" 1 ","description":" Old metadata "}"#, "sample").unwrap();
    assert_eq!((old.name.as_str(), old.version.as_str(), old.description.as_str()), ("Sample", "1", "Old metadata"));
    for mutate in [
        |m: &mut Value| m["runtime"]["module"] = json!("../other.wasm"),
        |m: &mut Value| m["runtime"]["commands"] = json!(["file.export"]),
        |m: &mut Value| m["runtime"]["commands"] = json!(["plugin.run"]),
        |m: &mut Value| m["runtime"]["entries"] = json!(["make", "make"]),
        |m: &mut Value| m["runtime"]["limits"]["memory_mib"] = json!(4097),
        |m: &mut Value| m["runtime"]["limits"]["fuel"] = json!(0),
        |m: &mut Value| m["runtime"]["limits"]["deadline_ms"] = json!(30_001),
        |m: &mut Value| m["runtime"]["api"] = json!("solidcraft:plugin@1.0.0"),
        |m: &mut Value| m["capabilities"] = json!(["commands@0.3"]),
        |m: &mut Value| m["capabilities"] = json!([]),
        |m: &mut Value| m["runtime"]["commands"] = json!(["solid.no_such_command"]),
        |m: &mut Value| m["runtime"]["permissions"] = json!({"command_groups":["network"]}),
        |m: &mut Value| m["network"] = json!(true),
        |m: &mut Value| m["permissions"] = json!(["net"]),
        |m: &mut Value| m["runtime"]["permissions"] = json!(["net"]),
        |m: &mut Value| m["runtime"]["permissions"] = json!({"net":true}),
    ] {
        let mut m = manifest();
        mutate(&mut m);
        assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_err(), "{m}");
    }
    assert_eq!(MAX_MANIFEST_BYTES, 64 * 1024);
    assert!(parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1], "sample").is_err());
}
#[test]
fn plugin_entries_cannot_shadow_host_commands() {
    let mut m = manifest();
    m["id"] = json!("solid");
    m["runtime"]["entries"] = json!(["box"]);
    let error = parse_manifest(&serde_json::to_vec(&m).unwrap(), "solid").unwrap_err();
    assert!(matches!(error.host_error(), Some(HostError::InvalidManifest { field, .. }) if field == "runtime.entries"), "{error}");
    assert!(error.to_string().contains("solid.box"));
}

fn panel_manifest() -> Value {
    let mut m = manifest();
    m["capabilities"] = json!(["commands@0.2", "ui@0.1"]);
    m["runtime"]["ui"] = json!({"ui_version":1,"panels":[{
        "id":"jobs","title":"Jobs","zones":["right"],"layout":{"root":0,"nodes":[{"widget":{"table":{
            "bind":"/jobs","row_id":"id","columns":[{"key":"name","label":"Operation","unit":null}],
            "select":"single","on_select":null,"on_edit":"make"
        }}}]}
    }],"workspaces":[]});
    m
}

fn panel_host() -> SolidCraftHost {
    let package = Package {
        manifest: parse_manifest(panel_manifest().to_string().as_bytes(), "sample").unwrap(),
        approval: "approved".into(),
        component: Vec::new(),
    };
    let mut session = Session::default();
    panels::register_package(&mut session, &package).unwrap();
    session.plugin_state.ui_caller = Some("sample".into());
    SolidCraftHost {
        session,
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    }
}

#[test]
fn panel_updates_are_namespaced_transactional_and_merged_once_per_frame() {
    let mut host = panel_host();
    let updates = [
        ui::Datum { path: "/jobs/0/id".into(), value: ui::UiValue::Text("op1".into()) },
        ui::Datum { path: "/jobs/0/name".into(), value: ui::UiValue::Text("Pocket".into()) },
    ];
    assert!(host.ui_set_data("jobs", &updates).is_err(), "updates need an active command transaction");
    host.begin_transaction().unwrap();
    assert!(host.ui_set_data("another-plugins-panel", &updates).is_err());
    host.ui_set_data("jobs", &updates).unwrap();
    host.commit_transaction().unwrap();
    assert!(host.session.plugin_panels()[0].data.rows("/jobs").is_empty());
    host.session.merge_plugin_panel_data();
    assert_eq!(host.session.plugin_panels()[0].data.rows("/jobs").len(), 1);
    assert_eq!(host.session.plugin_panels()[0].data.get("/jobs/0/name"), Some(&ui::UiValue::Text("Pocket".into())));
    host.begin_transaction().unwrap();
    host.ui_clear_data("jobs").unwrap();
    host.ui_show_panel("jobs", false).unwrap();
    host.rollback_transaction().unwrap();
    host.session.merge_plugin_panel_data();
    assert_eq!(host.session.plugin_panels()[0].data.rows("/jobs").len(), 1);
    assert!(host.session.plugin_panels()[0].visible);
    assert_eq!(host.session.plugin_panels()[0].visibility_revision, 0, "failed commands discard visibility requests");
    host.begin_transaction().unwrap();
    host.ui_show_panel("jobs", true).unwrap();
    host.ui_show_panel("jobs", true).unwrap();
    host.commit_transaction().unwrap();
    assert_eq!(host.session.plugin_panels()[0].visibility_revision, 2, "repeated show calls are distinct requests");
    let mut live = host.session;
    let prepared = live.scratch().compute_commands(&[("inspect.measure".into(), json!({}))], None).unwrap();
    panels::show_panel(Some(&mut live), "jobs", false).unwrap();
    live.accept_commands(prepared).unwrap();
    assert!(!live.plugin_panels()[0].visible, "a model worker must keep a panel closed meanwhile");
    live.replace_design(Session::default());
    assert!(live.plugin_panels()[0].data.rows("/jobs").is_empty(), "panel data belongs to the design that closed");
    live.disabled_plugins.insert("sample".into());
    assert!(live.plugin_panels().is_empty());
}

#[test]
fn panel_namespace_survives_design_replacement_within_a_component_transaction() {
    let mut host = panel_host();
    host.groups.insert("file".into());
    host.begin_transaction().unwrap();
    host.ui_set_data("jobs", &[ui::Datum { path: "/old".into(), value: ui::UiValue::Flag(true) }]).unwrap();
    host.run_command("file.new", &json!({"name":"New part"})).unwrap();
    host.ui_set_data("jobs", &[ui::Datum { path: "/new".into(), value: ui::UiValue::Flag(true) }]).unwrap();
    host.ui_show_panel("jobs", false).unwrap();
    host.commit_transaction().unwrap();
    host.session.merge_plugin_panel_data();
    let panel = host.session.plugin_panels().remove(0);
    assert_eq!(panel.data.get("/old"), None, "data from the closed design is cleared");
    assert_eq!(panel.data.get("/new"), Some(&ui::UiValue::Flag(true)));
    assert!(!panel.visible);
    host.session.replace_design(Session::default());
    assert_eq!(host.session.plugin_state.ui_caller, None, "a live design switch never keeps a guest call active");
}

#[test]
fn panel_outer_cancellation_restores_published_data_and_visibility() {
    let mut session = panel_host().session;
    panels::set_data(Some(&mut session), "jobs", &[ui::Datum { path: "/before".into(), value: ui::UiValue::Flag(true) }]).unwrap();
    session.merge_plugin_panel_data();
    let before = session.plugin_state.panels.clone();
    let command = crate::cmd::CommandSpec::new("plugin.run", "Run Plugin", |session, _| {
        panels::set_data(Some(&mut *session), "jobs", &[ui::Datum { path: "/after".into(), value: ui::UiValue::Flag(true) }])?;
        panels::show_panel(Some(&mut *session), "jobs", false)?;
        solidcraft_doc::progress::current().unwrap().cancel();
        Ok(json!({}))
    })
    .icon("play");
    let progress = std::sync::Arc::new(solidcraft_doc::progress::Progress::default());
    let result = solidcraft_doc::progress::watch(progress, || session.execute_spec(&command, &json!({})));
    assert!(matches!(result, Err(EngineError::Cancelled(_))), "{result:?}");
    assert_eq!(session.plugin_state.panels, before, "the outer cancellation discards data and visibility published by the runtime");
    session.merge_plugin_panel_data();
    assert_eq!(session.plugin_panels()[0].data.get("/after"), None);
}

#[test]
fn panel_actions_resolve_only_the_declared_entry_and_binding() {
    let panel_plugin = parse_manifest(panel_manifest().to_string().as_bytes(), "sample").unwrap();
    let event = json!({"panel":"jobs","event":"edit","path":"/jobs","row":"op1","value":null});
    let typed_event: ui::UiEvent = serde_json::from_value(event.clone()).unwrap();
    assert!(panels::event_for(&panel_plugin, "make", Some(&typed_event)).unwrap().is_some());
    assert!(panels::event_for(&panel_plugin, "delete", Some(&typed_event)).is_err());
    for change in [json!({"panel":"other"}), json!({"path":"/tools"}), json!({"event":"press"})] {
        let mut bad = event.clone();
        for (key, value) in change.as_object().unwrap() {
            bad[key] = value.clone();
        }
        let bad = serde_json::from_value(bad).unwrap();
        assert!(panels::event_for(&panel_plugin, "make", Some(&bad)).is_err(), "{bad:?}");
    }
    assert!(panels::event_for(&panel_plugin, "make", None).unwrap().is_none());
    let ordinary = parse_manifest(manifest().to_string().as_bytes(), "sample").unwrap();
    assert!(panels::event_for(&ordinary, "make", None).unwrap().is_none());
    let refused = panels::event_for(&ordinary, "make", Some(&typed_event)).unwrap_err();
    assert!(matches!(refused.host_error(), Some(HostError::InvalidInput { field, .. }) if field == "ui_event"));
}

#[test]
fn panel_named_calls_keep_the_explicit_event_envelope_separate_from_entry_params() {
    let mut session = Session::default();
    session.plugin_state.packages.insert("sample".into(), "sample-folder".into());
    let event = json!({"panel":"jobs","event":"edit","path":"/jobs","row":"op1","value":null});
    let params = json!({"panel":"application-domain-panel","event":"rebuild"});
    let call = resolve_call(&session, "sample.make", &json!({"approval":"reviewed","params":params,"ui_event":event})).unwrap().unwrap();
    assert_eq!(call["params"], params);
    assert_eq!(call["ui_event"], event);
}

#[test]
fn capability_refusals_are_plain_and_name_solidcraft() {
    let refusal = |capabilities: Value| {
        let mut m = manifest();
        m["capabilities"] = capabilities;
        parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").unwrap_err().to_string()
    };
    assert_eq!(refusal(json!(["commands@0.3"])), "plugin: needs commands 0.3; update SolidCraft or the plugin");
    assert!(refusal(json!(["commands@0.1", "viewport@0.1"])).contains("needs viewport 0.1, which this version of SolidCraft does not have"));
    let mut old = manifest();
    old["runtime"]["api"] = json!("solidcraft:plugin@1.0.0");
    let old = parse_manifest(&serde_json::to_vec(&old).unwrap(), "sample").unwrap_err().to_string();
    assert!(old.contains("was built for solidcraft:plugin@1.0.0") && old.contains("update the plugin"), "{old}");
}
#[test]
fn an_importer_checks_its_own_design_apart_from_the_open_one() {
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    };
    let mut made = Session::new(solidcraft_doc::Document::new("Bracket"));
    made.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    host.evaluate_open(&solidcraft_io::write_design(&made.doc).unwrap()).unwrap();
    let measured = host.evaluate_run("inspect.measure", &json!({})).unwrap();
    assert_eq!(measured["bodies"].as_array().map(Vec::len), Some(1), "{measured}");
    assert!(host.evaluate_run("document.inspect", &json!({})).unwrap()["timeline"].is_array());
    // A command that fails is an answer; a command outside the list is refused.
    let missing = host.evaluate_run("model.edges", &json!({"body":"No such body"})).unwrap();
    assert!(missing["error"].is_string(), "{missing}");
    for refused in ["solid.box", "file.export_stl", "doc.open", "plugin.install"] {
        assert!(matches!(host.evaluate_run(refused, &json!({})), Err(HostError::PermissionDenied { .. })), "{refused}");
    }
    assert!(host.session.doc.features.is_empty(), "the open design is untouched");
    host.evaluate_close();
    assert!(host.evaluate_run("inspect.measure", &json!({})).is_err(), "nothing stays open");
    assert!(matches!(host.evaluate_open(b"not a design"), Err(HostError::InvalidInput { .. })));
}

#[test]
fn every_built_command_is_reachable_and_sensitive_ones_need_their_group() {
    let host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    };
    let catalog = host.command_catalog();
    let built = crate::command_specs().into_iter().filter(|c| c.planned.is_none()).count();
    assert_eq!(catalog.len(), built, "the plugin catalog is the whole MCP catalog");
    let group = |id: &str| catalog.iter().find(|c| c.name == id).unwrap_or_else(|| panic!("{id} is in the catalog")).group.clone();
    for (id, expected) in [
        ("solid.box", None),
        ("sketch.line", None),
        ("timeline.inspect", None),
        ("parameters.change", None),
        ("file.save", Some("file")),
        ("file.export_step", Some("file")),
        ("sketch.insert_dxf", Some("file")),
        ("canvas.insert", Some("file")),
        ("library.export", Some("file")),
        ("doc.open", Some("doc")),
        ("settings.get", Some("app")),
        ("plugin.run", Some("plugin")),
    ] {
        if crate::find_command(id).is_some() {
            assert_eq!(group(id).as_deref(), expected, "{id}");
        }
    }
    for c in &catalog {
        if let Some(g) = &c.group {
            assert!(COMMAND_GROUPS.contains(&g.as_str()), "{}: {g}", c.name);
        }
    }
    // A manifest names the group for each sensitive command it lists; then it loads.
    let mut m = manifest();
    m["runtime"]["commands"] = json!(["solid.box", "file.export_step"]);
    let refused = parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").unwrap_err().to_string();
    assert!(refused.contains("needs \"file\" in runtime.permissions.command_groups"), "{refused}");
    m["runtime"]["permissions"] = json!({"command_groups":["file"]});
    assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_ok());
}
/// A read-only folder must pass the importer's scope check without granting write access.
/// Both the user's grant and the package's manifest must allow the intended operation.
#[test]
fn file_scopes_require_separate_read_and_write_approvals() {
    let folder = solidcraft_io::vfs::temp_dir().join("sc-readonly-grant-18793");
    let approved = vec![FileGrant { name: "data".into(), folder: folder.clone(), read: true, write: false }];
    let manifest = vec![craft_plugin::FilePermission { name: "data".into(), read: true, write: false }];
    assert_eq!(approved_file_folders(&approved, &manifest, false), vec![folder.clone()]);
    assert!(approved_file_folders(&approved, &manifest, true).is_empty());

    let mut more_grant = approved.clone();
    more_grant[0].write = true;
    assert!(approved_file_folders(&more_grant, &manifest, true).is_empty(), "a grant cannot override a read-only manifest");
    let mut more_manifest = manifest.clone();
    more_manifest[0].write = true;
    assert!(approved_file_folders(&approved, &more_manifest, true).is_empty(), "the manifest cannot override read-only approval");
    assert_eq!(approved_file_folders(&more_grant, &more_manifest, true), vec![folder.clone()]);
    assert!(approved_file_folders(&more_grant, &[], false).is_empty(), "a missing manifest declaration confers no access");
    assert!(approved_file_folders(&[], &more_manifest, false).is_empty(), "a missing user grant confers no access");

    for command in [
        "component.insert_design",
        "file.insert_step",
        "file.insert_mesh",
        "file.insert_gerber",
        "file.gerber_files",
        "sketch.insert_dxf",
        "sketch.insert_svg",
    ] {
        assert!(input_only_command(command), "{command} reads its supplied file");
    }
    for command in ["file.save_as", "file.copy", "file.move", "file.trash", "file.export_step", "doc.open", "doc.recovery_discard", "library.export"]
    {
        assert!(!input_only_command(command), "{command} needs the stricter write-capable scope");
    }
}

/// Importing a STEP may change the design without modifying its source file. Even after this
/// read-only command succeeds, write commands must not gain access to the same folder.
#[test]
fn read_only_folder_allows_step_insert_but_refuses_file_writes() {
    let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-read-scope-{}", std::process::id()));
    let read = root.join("read");
    std::fs::create_dir_all(&read).unwrap();
    let source_path = read.join("source.step");
    let mut source = Session::default();
    source.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    source.execute("file.export_step", &json!({"path":source_path})).unwrap();
    assert!(source_path.is_file());

    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::from(["file".into()]),
        file_folders: Vec::new(),
        file_read_folders: vec![read.clone()],
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    };
    host.begin_transaction().unwrap();
    let inserted = host.run_command("file.insert_step", &json!({"path":source_path}));
    assert!(inserted.is_ok(), "read-only insert should work: {inserted:?}");
    let output = read.join("forbidden.sc");
    let write = host.run_command("file.save_as", &json!({"path":output}));
    assert!(matches!(&write, Err(HostError::CommandFailed { .. })), "the same grant cannot save files: {write:?}");
    assert!(!output.exists(), "no file was written under a read-only grant");
    let output = read.join("forbidden.stl");
    assert!(host.run_command("file.export_stl", &json!({"path":output})).is_err(), "export also requires write authority");
    assert!(!output.exists());
    assert!(source_path.exists(), "the imported STEP is unchanged");
    host.rollback_transaction().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sensitive_command_is_refused_unless_granted_and_a_granted_file_command_sees_only_its_folders() {
    let root = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-plugin-groups-{}", std::process::id()));
    let granted = root.join("granted");
    std::fs::create_dir_all(&granted).unwrap();
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    };
    host.begin_transaction().unwrap();
    s_box(&mut host);
    let save = |host: &mut SolidCraftHost, path: &std::path::Path| host.run_command("file.save_as", &json!({"path": path.to_string_lossy()}));
    let refused = save(&mut host, &granted.join("a.sc")).unwrap_err();
    assert!(matches!(&refused, HostError::PermissionDenied { permission } if permission.contains("`file` permission")), "{refused}");
    for id in ["doc.open", "plugin.list"] {
        assert!(matches!(host.run_command(id, &json!({})), Err(HostError::PermissionDenied { .. })), "{id}");
    }
    host.groups.insert("file".into());
    host.file_folders = vec![granted.clone()];
    save(&mut host, &granted.join("a.sc")).unwrap();
    assert!(granted.join("a.sc").is_file());
    let outside = save(&mut host, &root.join("outside.sc")).unwrap_err();
    assert!(matches!(outside, HostError::CommandFailed { .. }), "{outside}");
    assert!(!root.join("outside.sc").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
#[test]
fn nested_plugin_edits_save_their_current_token_without_retagging_at_acceptance() {
    let folder = crate::tests::Folder::new("nested-plugin-save-token");
    let path = folder.path("edited.sc");
    let mut host = SolidCraftHost {
        forms: Vec::new(),
        form_source: None,
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::from(["file".to_string()]),
        file_folders: vec![folder.0.clone()],
        file_read_folders: Vec::new(),
        evaluation: None,
    };
    let before = host.session.design_token();
    let mut accepted = Session::default();
    let identity = accepted.document_id.clone();
    // The real plugin entry owns this outer frame and one undo transaction.
    let (_frame, outermost) = crate::frames::Nested::enter("plugin.run");
    assert!(outermost);
    host.begin_transaction().unwrap();
    host.run_command("parameters.add", &json!({"name": "width", "expression": "10 mm"})).unwrap();
    let added = host.transaction.as_ref().unwrap().design_token();
    assert_ne!(added, before);
    host.run_command("parameters.change", &json!({"name": "width", "expression": "20 mm"})).unwrap();
    let edited = host.transaction.as_ref().unwrap().design_token();
    assert_ne!(edited, added, "each nested mutation receives a distinct token");
    host.run_command("file.save_as", &json!({"path": path})).unwrap();
    let saved = solidcraft_io::read_design(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.to_json(), host.transaction.as_ref().unwrap().doc.to_json());
    assert_eq!(host.transaction.as_ref().unwrap().design_token(), edited);
    host.commit_transaction().unwrap();
    assert_eq!(host.session.design_token(), edited);
    accepted.accept_computed(host.session).unwrap();
    assert_eq!(accepted.design_token().revision, edited.revision);
    assert_eq!(accepted.design_token().document_id, identity);
}

/// A plugin reaches a command only by its exact registered id: another
/// case, whitespace around it or a look-alike letter is not a granted command, so it can't slip
/// past a command group, nor a manifest list a command under another spelling.
#[test]
fn a_command_in_another_spelling_is_never_granted() {
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    };
    host.begin_transaction().unwrap();
    for id in ["file.save_as", "file.export_step", "doc.open", "plugin.run", "plugin.list", "settings.get", "diagnostics.crash_reports", "solid.box"]
    {
        let mixed: String = id.chars().enumerate().map(|(i, c)| if i % 2 == 0 { c.to_ascii_uppercase() } else { c }).collect();
        for spelled in [
            id.to_ascii_uppercase(),
            mixed,
            format!(" {id}"),
            format!("{id}\n"),
            format!("\u{a0}{id}"),
            id.replacen('o', "\u{043e}", 1),
            id.replacen('.', "\u{ff0e}", 1),
            format!("\u{200b}{id}"),
        ]
        .into_iter()
        .filter(|spelled| spelled != id)
        {
            let refused = host.run_command(&spelled, &json!({"path": "x.sc", "length": 1, "width": 1, "height": 1}));
            assert!(
                matches!(&refused, Err(HostError::PermissionDenied { permission }) if permission.contains("is not granted")),
                "{spelled:?}: {refused:?}"
            );
            let mut m = manifest();
            m["runtime"]["commands"] = json!([spelled.clone()]);
            m["runtime"]["permissions"] = json!({"command_groups": ["app", "doc", "file", "plugin"]});
            assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_err(), "manifest lists {spelled:?}");
        }
    }
    let catalog = host.command_catalog();
    assert!(catalog.iter().all(|c| crate::find_command(&c.name).is_some_and(|spec| spec.id == c.name)), "the catalog lists registered ids");
}
fn s_box(host: &mut SolidCraftHost) {
    host.run_command("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
}
#[test]
fn disabled_ids_survive_scratch_and_new_design() {
    let mut s = Session::default();
    s.disabled_plugins.insert("sample".into());
    assert!(s.scratch().disabled_plugins.contains("sample"));
    s.execute("file.new", &json!({})).unwrap();
    assert!(s.disabled_plugins.contains("sample"));
}

#[test]
fn a_failure_message_shows_braces_in_a_file_name_or_reason_as_they_are() {
    let failure = |file: &str, reason: &str| PluginFailure {
        plugin: "Sample".into(),
        file: Some(file.into()),
        exporting: false,
        error: craft_plugin::HostError::Unsupported { reason: reason.into() },
    };
    assert_eq!(failure("part{reason}.smp", "no {file} solids").text(), "Sample does not support part{reason}.smp: no {file} solids");
    assert_eq!(failure("part.smp", "no solids").text(), "Sample does not support part.smp: no solids");
}
#[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod native {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(code: &str) -> Self {
            let root =
                solidcraft_io::vfs::temp_dir().join(format!("solidcraft-plugin-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let dir = root.join("sample");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plugin.json"), serde_json::to_vec(&manifest()).unwrap()).unwrap();
            std::fs::write(dir.join("plugin.wasm"), component(code)).unwrap();
            Self(root)
        }
        fn path(&self) -> String {
            self.0.join("sample").to_string_lossy().into_owned()
        }
        fn run(&self, s: &mut Session) -> crate::Result<Value> {
            let approval = s.execute("plugin.inspect", &json!({"path":self.path()}))?["approval"].clone();
            s.execute("plugin.run", &json!({"path":self.path(),"approval":approval,"entry":"make","params":{}}))
        }
        fn install(&self, s: &mut Session, approve: Value) -> String {
            let bytes = solidcraft_io::safe_zip::write(&[
                ("plugin.json", &std::fs::read(self.0.join("sample/plugin.json")).unwrap()),
                ("plugin.wasm", &std::fs::read(self.0.join("sample/plugin.wasm")).unwrap()),
            ])
            .unwrap();
            let zip = self.0.join("package.zip");
            std::fs::write(&zip, bytes).unwrap();
            s.plugin_state.plugins_dir = Some(self.0.join("installed"));
            s.execute("plugin.install", &json!({"path":zip,"replace":true,"approve":approve})).unwrap()["folder"].as_str().unwrap().into()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // An original component fixture. A real canonical lift/lower crosses the host boundary.
    // $box asks for a 2 x 3 x 4 mm solid; memory at 0 is the canonical result area.
    fn component_template() -> String {
        let (package, version) = API.split_once('@').unwrap();
        include_str!("fixture.wat").replace("\r\n", "\n").replace("__PACKAGE__", package).replace("__VERSION__", version)
    }
    fn component(code: &str) -> Vec<u8> {
        let template = component_template();
        let original = "      call $box\n      call $box\n";
        assert!(template.contains(original));
        wat::parse_str(template.replacen(original, &format!("      {code}\n"), 1)).unwrap()
    }
    fn panel_fixture() -> Fixture {
        let fixture = Fixture::new("call $box");
        let manifest = json!({"id":"sample","name":"Sample","version":"1","capabilities":["commands@0.2","ui@0.1"],"runtime":{
            "module":"plugin.wasm","commands":["inspect.measure"],"entries":[{"name":"make","effect":"read"}],
            "limits":{"fuel":1000000,"memory_mib":2,"deadline_ms":5000},
            "ui":{"ui_version":1,"panels":[{"id":"jobs","title":"Jobs","zones":["bottom"],"layout":{"root":0,"nodes":[{"widget":{"table":{
                "bind":"/jobs","row_id":"id","columns":[{"key":"state","label":"state","unit":null}],"select":"single","on_select":"make","on_edit":"make"
            }}}]}}],"workspaces":[]}
        }});
        let (package, version) = API.split_once('@').unwrap();
        let source = include_str!("../../../craft-plugin-wasm/src/ui_fixture.wat")
            .replace("__PACKAGE__", package)
            .replace("__VERSION__", version)
            .replace("__COMMAND__", "inspect.measure")
            .replace("__COMMAND_LEN__", "15")
            .replace("__ARGS__", "{}")
            .replace("__ARGS_LEN__", "2");
        std::fs::write(fixture.0.join("sample/plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(fixture.0.join("sample/plugin.wasm"), wat::parse_str(source).unwrap()).unwrap();
        fixture
    }
    #[test]
    fn panel_event_shaped_entry_params_remain_ordinary_inputs() {
        let fixture = panel_fixture();
        let mut session = Session::default();
        let approval = session.execute("plugin.inspect", &json!({"path":fixture.path()})).unwrap()["approval"].clone();
        let event = json!({"panel":"jobs","event":"select","path":"/jobs","row":"op1","value":null});
        session.execute("sample.make", &json!({"approval":approval,"params":event})).unwrap();
        session.merge_plugin_panel_data();
        assert_eq!(
            session.plugin_panels()[0].data.get("/jobs/0/state"),
            Some(&ui::UiValue::Text("ready".into())),
            "ordinary fields never call ui-provider.on-event"
        );
        session.execute("sample.make", &json!({"approval":approval,"params":event,"ui_event":event})).unwrap();
        session.merge_plugin_panel_data();
        assert_eq!(
            session.plugin_panels()[0].data.get("/jobs/0/state"),
            Some(&ui::UiValue::Text("posted".into())),
            "the explicit envelope delivers on-event before the entry"
        );
    }
    #[test]
    fn panel_frame_merges_keep_plugin_workers_current_and_grant_changes_stale() {
        let fixture = panel_fixture();
        let mut session = Session::default();
        fixture.run(&mut session).unwrap();
        let package = read_package(&fixture.path()).unwrap();
        let request = json!({"path":fixture.path(),"approval":package.approval,"entry":"make"});
        let prepared = session.scratch().compute_commands(&[("plugin.run".into(), request.clone())], None).unwrap();
        assert!(session.plugin_panels()[0].data.values().is_empty(), "successful data is pending before the frame");
        session.merge_plugin_panel_data();
        assert!(prepared.is_current(&session), "frame adoption changes presentation only");
        session.accept_commands(prepared).unwrap();
        assert_eq!(
            session.plugin_panels()[0].data.get("/jobs/0/state"),
            Some(&ui::UiValue::Text("ready".into())),
            "an unchanged worker keeps the merged live frame"
        );
        let prepared = session.scratch().compute_commands(&[("plugin.run".into(), request)], None).unwrap();
        session.plugin_state.grants.insert(
            "sample".into(),
            Grants { package_hash: Some(package.approval), command_groups: BTreeSet::from(["file".into()]), ..Default::default() },
        );
        assert!(!prepared.is_current(&session), "a real approval change still invalidates plugin work");
        assert!(session.accept_commands(prepared).is_err());
    }
    #[test]
    fn panel_data_changes_invalidate_computed_plugin_work() {
        let fixture = panel_fixture();
        let mut session = Session::default();
        fixture.run(&mut session).unwrap();
        let package = read_package(&fixture.path()).unwrap();
        let request = json!({"path":fixture.path(),"approval":package.approval,"entry":"make"});
        let prepared = session.scratch().compute_commands(&[("plugin.run".into(), request)], None).unwrap();
        session
            .plugin_state
            .panels
            .get_mut("sample")
            .unwrap()
            .data
            .get_mut("jobs")
            .unwrap()
            .set_data(&[ui::Datum { path: "/jobs/0/state".into(), value: ui::UiValue::Text("newer".into()) }])
            .unwrap();
        assert!(!prepared.is_current(&session), "newer logical panel data must not be overwritten by stale plugin work");
        assert!(session.accept_commands(prepared).is_err());
        session.merge_plugin_panel_data();
        assert_eq!(session.plugin_panels()[0].data.get("/jobs/0/state"), Some(&ui::UiValue::Text("newer".into())));
    }
    fn file_component(write: bool) -> Vec<u8> {
        file_component_path(write, "secret.txt")
    }
    fn file_component_path(write: bool, path: &str) -> Vec<u8> {
        let path_len = path.len();
        let (package, version) = API.split_once('@').unwrap();
        let template = component_template();
        // Keep exactly the shared WIT types, then exercise the real files interface.
        let (types, _) = template.split_once(&format!("  (type $ty-{package}/commands@{version}")).unwrap();
        let (operation, result, parameters, arguments) = if write {
            ("write-file", "(result (error $error))", "(param \"data\" (list u8))", "i32.const 128 i32.const 7")
        } else {
            ("read-file", "(result (list u8) (error $error))", "", "")
        };
        let core_parameters = if write { "i32 i32 i32 i32 i32 i32 i32" } else { "i32 i32 i32 i32 i32" };
        let source = format!(
            r#"{types}
  (type $files-type (instance
    (alias outer 1 $plugin-error (type $error))
    (export "plugin-error" (type (eq $error)))
    (type $file-result {result})
    (type $operation (func (param "grant" string) (param "path" string) {parameters} (result $file-result)))
    (export "{operation}" (func (type $operation)))
  ))
  (import "{package}/files@{version}" (instance $files (type $files-type)))
  (alias export $files "{operation}" (func $file))
  (core module $allocation
    (memory (export "memory") 1)
    (global $heap (mut i32) i32.const 4096)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $at i32)
      global.get $heap local.tee $at local.get 3 i32.add global.set $heap local.get $at)
  )
  (core instance $allocation (instantiate $allocation))
  (alias core export $allocation "memory" (core memory $memory))
  (alias core export $allocation "realloc" (core func $realloc))
  (core func $file (canon lower (func $file) (memory $memory) (realloc $realloc)))
  (core instance $imports (export "file" (func $file)) (export "memory" (memory $memory)))
  (core module $main
    (import "host" "file" (func $file (param {core_parameters})))
    (import "host" "memory" (memory 1))
    (data (i32.const 64) "project")
    (data (i32.const 96) "{path}")
    (data (i32.const 128) "changed")
    (func (export "execute") (param i32 i32 i32 i32) (result i32)
      i32.const 64 i32.const 7 i32.const 96 i32.const {path_len} {arguments} i32.const 0 call $file
      i32.const 0)
  )
  (core instance $main (instantiate $main (with "host" (instance $imports))))
  (alias core export $main "execute" (core func $execute))
  (func $execute (param "entry" string) (param "params" $json) (result (result $json (error $plugin-error)))
    (canon lift (core func $execute) (memory $memory) (realloc $realloc)))
  (instance $command
    (export "json" (type $json))
    (export "plugin-error" (type $plugin-error))
    (export "execute" (func $execute)))
  (export "{package}/entry@{version}" (instance $command))
)"#
        );
        wat::parse_str(source).unwrap()
    }
    #[test]
    fn larger_declared_budgets_need_user_ceilings_and_exact_content_approval() {
        let f = Fixture::new("call $box i32.const 2048 memory.grow i32.const -1 i32.eq if unreachable end");
        let path = f.0.join("sample/plugin.json");
        let mut m = manifest();
        m["runtime"]["limits"] = json!({"fuel":200_000_000_000_u64,"memory_mib":256,"deadline_ms":5000});
        std::fs::write(&path, m.to_string()).unwrap();
        let mut s = Session::default();
        let inspection = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        assert_eq!(inspection["within_host_limits"], false);
        assert_eq!(inspection["host_limits"]["max_memory_mib"], 128);
        let refusal = "Sample: it asks for 200000000000 fuel, and plugins may use 100000000000. Install the plugin again and allow its fuel request, or raise Max plugin fuel in Preferences › Plugins (in solidcraft-cli, max_fuel in the --plugin-limits file).";
        assert_eq!(inspection["limits_error"], refusal);
        let before = s.doc.to_json();
        let error = f.run(&mut s).unwrap_err();
        assert_eq!(error.to_string(), refusal);
        assert_eq!(error.host_error(), Some(&HostError::OverCeiling { limit: "fuel".into(), requested: 200_000_000_000, allowed: 100_000_000_000 }));
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
        s.plugin_limits = HostLimits { max_fuel: 200_000_000_000, max_memory_mib: 256 };
        let approved = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        assert_eq!(approved["within_host_limits"], true);
        assert_eq!(approved["approval"], inspection["approval"], "approval pins the exact request; a ceiling change never changes its budget");
        f.run(&mut s).unwrap();
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 24.);
        assert_eq!(s.undo.len(), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        // Even the largest u64 budget reaches Wasmtime exactly; a new request needs new approval.
        m["runtime"]["limits"]["fuel"] = json!(u64::MAX);
        std::fs::write(&path, m.to_string()).unwrap();
        s.plugin_limits.max_fuel = u64::MAX;
        assert!(s.execute("plugin.run", &json!({"path":f.path(),"approval":approved["approval"],"entry":"make"})).is_err());
        assert_eq!(s.doc.to_json(), before);
        f.run(&mut s).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        // Reducing the host policy blocks a previously reviewed package.
        s.plugin_limits = HostLimits::default();
        assert!(f.run(&mut s).is_err());
        assert_eq!(s.doc.to_json(), before);
    }
    #[test]
    fn approved_fuel_runs_without_changing_other_plugins_default_limit() {
        let f = Fixture::new("call $box");
        let path = f.0.join("sample/plugin.json");
        let mut m = manifest();
        let fuel = 10_000_000_000_000_u64;
        m["runtime"]["limits"]["fuel"] = json!(fuel);
        std::fs::write(&path, m.to_string()).unwrap();
        let mut s = Session::default();
        let installed = f.install(&mut s, json!({"fuel":fuel}));
        let inspection = s.execute("plugin.inspect", &json!({"path":installed})).unwrap();
        assert_eq!(inspection["within_host_limits"], true);
        s.execute("plugin.run", &json!({"path":installed,"approval":inspection["approval"],"entry":"make"})).unwrap();
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 24.);
        m["runtime"]["limits"]["fuel"] = json!(fuel - 1);
        std::fs::write(Path::new(&installed).join("plugin.json"), m.to_string()).unwrap();
        let changed = s.execute("plugin.inspect", &json!({"path":installed})).unwrap();
        assert_eq!(changed["within_host_limits"], false);
        assert!(s.execute("plugin.run", &json!({"path":installed,"approval":changed["approval"],"entry":"make"})).is_err());
        // An omitted limits object still uses the manifest's default fuel under Max plugin fuel.
        m["runtime"].as_object_mut().unwrap().remove("limits");
        std::fs::write(&path, m.to_string()).unwrap();
        let installed = f.install(&mut s, json!({}));
        let approval = read_package(&installed).unwrap().approval;
        assert_eq!(host_limits_for(&s, "sample", &approval).max_fuel, HostLimits::default().max_fuel);
        s.execute("plugin.run", &json!({"path":installed,"approval":approval,"entry":"make"})).unwrap();
    }

    #[test]
    fn a_plugin_prepared_under_an_old_host_policy_is_not_adopted() {
        for command in ["plugin.run", "PLUGIN.RUN"] {
            let f = Fixture::new("call $box");
            let mut s = Session::default();
            let i = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
            let params = json!({"path":f.path(),"approval":i["approval"],"entry":"make"});
            let prepared = s.scratch().compute_commands(&[(command.into(), params)], None).unwrap();
            assert!(prepared.is_current(&s));
            s.plugin_limits.max_memory_mib = 512;
            assert!(!prepared.is_current(&s));
            let before = s.doc.to_json();
            assert!(s.accept_commands(prepared).is_err());
            assert_eq!(s.doc.to_json(), before);
            assert!(s.undo.is_empty());
        }
    }
    #[test]
    fn component_edits_are_one_undo_step_and_reopen_without_the_plugin() {
        let f = Fixture::new("call $box call $box");
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
        let before = s.doc.to_json();
        let undo = s.undo.len();
        f.run(&mut s).unwrap();
        assert_eq!(s.doc.features.len(), 3);
        assert_eq!(s.undo.len(), undo + 1);
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 72.);
        let saved = s.doc.to_json();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo, "one undo removes the entire plugin edit");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), saved);
        assert_eq!(s.undo.len(), undo + 1);
        drop(f);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert_eq!(reopened.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 72.);
    }
    #[test]
    fn component_file_reads_and_writes_need_a_host_grant_even_when_declared() {
        for write in [false, true] {
            for declared in [false, true] {
                let f = Fixture::new("call $box");
                let secret = f.0.join("sample/secret.txt");
                std::fs::write(&secret, b"private contents").unwrap();
                std::fs::write(f.0.join("sample/plugin.wasm"), file_component(write)).unwrap();
                if declared {
                    let mut m = manifest();
                    m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":true}]});
                    std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
                }
                let mut s = Session::default();
                s.execute("solid.box", &json!({"length":1,"width":2,"height":3})).unwrap();
                let before = s.doc.to_json();
                let history = (s.undo.len(), s.redo.len());
                assert!(s.plugin_state.file_grants.is_empty());
                let error = f.run(&mut s).unwrap_err();
                let expected = if write { "files.write:project" } else { "files.read:project" };
                assert!(
                    matches!(error.host_error(), Some(craft_plugin::HostError::PermissionDenied { permission }) if permission == expected),
                    "write={write}, declared={declared}: {error}"
                );
                assert_eq!(std::fs::read(&secret).unwrap(), b"private contents");
                assert_eq!(s.doc.to_json(), before);
                assert_eq!((s.undo.len(), s.redo.len()), history);
                assert_eq!(solidcraft_io::vfs::current_scope(), None);
            }
        }
    }
    #[test]
    fn nested_plugin_failure_restores_the_callers_session_token_and_authored_bytes() {
        let f = Fixture::new("call $box unreachable");
        let mut s = Session::default();
        let token = s.design_token();
        let before = s.doc.to_json();
        assert!(f.run(&mut s).is_err());
        assert_eq!(s.design_token(), token);
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty() && s.redo.is_empty());
    }

    #[test]
    fn traps_fuel_memory_and_denied_calls_leave_design_and_history_untouched() {
        for (code, diagnostic) in [
            ("call $box unreachable", "trap=UnreachableCodeReached"),
            ("call $box i32.const 8 i32.const 1 i32.store", "entry result is not JSON"),
            (
                "call $box (loop $forever br $forever)",
                "Sample: it used all 100000 fuel it asks for and was stopped. Ask its author to raise its fuel limit.",
            ),
            ("call $box i32.const 10000 memory.grow drop", "Sample: it needed more than the 1 MiB of memory it asks for and was stopped."),
            ("call $box i32.const -1 i32.load drop", "trap=MemoryOutOfBounds"),
            ("call $box i32.const 1 i32.const 0 i32.div_u drop", "trap=IntegerDivisionByZero"),
            ("call $box i32.const 2048 i32.const 11 i32.const 1024 i32.const 33 i32.const 0 call $run", "not granted"),
            (
                "call $box i32.const 2048 i32.const 11 i32.const 1024 i32.const 33 i32.const 0 call $run i32.const 0 i32.const 0 i32.store i32.const 4 i32.const 1024 i32.store i32.const 8 i32.const 33 i32.store",
                "not granted",
            ),
        ] {
            let f = Fixture::new(code);
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            let before = s.doc.to_json();
            let history = (s.undo.len(), s.redo.len());
            let message = f.run(&mut s).unwrap_err().to_string();
            assert!(message.contains(diagnostic), "{code}: {message}");
            assert!(message.starts_with("Sample"), "a plugin's failure names it first: {message}");
            assert_eq!(s.doc.to_json(), before, "{code}");
            assert_eq!((s.undo.len(), s.redo.len()), history, "{code}");
            assert_eq!(s.active_sketch, None);
            assert_eq!(solidcraft_io::vfs::current_scope(), None);
        }
    }
    #[test]
    fn a_granted_component_reads_only_the_evaluated_mesh_and_preserves_history() {
        let f = Fixture::new("call $box");
        let template = component_template();
        let old_args = r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#;
        assert!(template.contains(old_args));
        assert_eq!(template.matches("i32.const 9\n").count(), 1);
        assert_eq!(template.matches("i32.const 33\n").count(), 1);
        let wat = template
            .replace("      call $box\n      call $box\n", "      call $box\n")
            .replace("solid.box", "model.mesh")
            .replace(old_args, r#"{\22body\22:\22Box\22}"#)
            .replace("i32.const 9\n", "i32.const 10\n")
            .replace("i32.const 33\n", "i32.const 14\n");
        std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(wat).unwrap()).unwrap();
        let mut m = manifest();
        m["runtime"]["commands"] = json!(["model.mesh"]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":2,"width":3,"height":4,"body_name":"Box"})).unwrap();
        let before = s.doc.to_json();
        let history = (s.undo.len(), s.redo.len());
        let result = f.run(&mut s).unwrap();
        assert_eq!(result["result"]["frame"], "component-local-mm");
        assert!(!result["result"]["triangles"].as_array().unwrap().is_empty());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!((s.undo.len(), s.redo.len()), history);
    }

    #[test]
    fn approval_binds_exact_bytes_and_disabled_plugins_never_execute() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let inspected = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let args = json!({"path":f.path(),"approval":inspected["approval"],"entry":"make"});
        let path = f.0.join("sample/plugin.json");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.push(b' ');
        std::fs::write(&path, bytes).unwrap();
        assert!(s.execute("plugin.run", &args).unwrap_err().to_string().contains("approval"));
        s.disabled_plugins.insert("sample".into());
        assert!(f.run(&mut s).unwrap_err().to_string().contains("disabled"));
        assert!(s.doc.features.is_empty());
        solidcraft_io::vfs::scoped(Some(Default::default()), || {
            assert!(s.execute("plugin.inspect", &json!({"path":f.path()})).is_err());
        });
    }

    #[test]
    fn net_manifest_permission_is_refused_before_execution() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"net":true});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let before = s.doc.to_json();
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(error.host_error(), Some(craft_plugin::HostError::PermissionDenied { permission }) if permission == "net"));
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }

    #[test]
    fn sockets_and_http_imports_are_absent_from_the_linker() {
        let (package, version) = API.split_once('@').unwrap();
        for import in [
            "wasi:sockets/network@0.2.0".to_string(),
            "wasi:http/outgoing-handler@0.2.0".into(),
            format!("{package}/fetch@{version}"),
            "fetch".into(),
        ] {
            let f = Fixture::new("call $box");
            let template = component_template();
            let (prefix, _) = template.rsplit_once(')').unwrap();
            let wat = format!("{prefix}\n(type $network-type (instance))\n(import \"{import}\" (instance $network (type $network-type)))\n)");
            std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(wat).unwrap()).unwrap();
            let mut s = Session::default();
            let before = s.doc.to_json();
            let error = f.run(&mut s).unwrap_err();
            assert!(matches!(error.host_error(), Some(craft_plugin::HostError::PermissionDenied { .. })), "{import}: {error}");
            assert!(error.to_string().contains(&import), "{error}");
            assert_eq!(s.doc.to_json(), before);
            assert!(s.undo.is_empty());
        }
    }

    fn dialog_fixture(code: &str) -> Fixture {
        let f = Fixture::new("");
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.1", "dialog@0.1"]);
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","dialog_hook":true,"input_schema":{
        "type":"object","required":["width"],"properties":{
            "width":{"type":"number","minimum":1},
            "size":{"type":"string","oneOf":[{"const":"small"},{"const":"large"}]}
        }}}]);
        m["runtime"]["limits"] = json!({"fuel":10000000,"memory_mib":1,"deadline_ms":250});
        let reply = r#"{"width":{"default":24},"size":{"choices":["large"]}}"#;
        let bytes = include_str!("../../../craft-plugin-wasm/src/dialog_fixture.wat")
            .replace("__PACKAGE__", "craft:plugin")
            .replace("__VERSION__", "0.2.0")
            .replace("__HOOK_CODE__", code)
            .replace("__REPLY_LEN__", &reply.len().to_string())
            .replace("__REPLY__", &reply.replace('"', "\\22"));
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(bytes).unwrap()).unwrap();
        f
    }

    #[test]
    fn plugin_run_dialog_hook_fills_omissions_preserves_callers_and_rejects_narrowed_choices() {
        let f = dialog_fixture("");
        let mut s = Session::default();
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        let args = |params: Value| json!({"path":f.path(),"approval":approval,"entry":"make","params":params});
        let tools = s.execute("plugin.tools", &json!({"name":"sample.make"})).unwrap();
        let schema = &tools["tools"][0]["input_schema"];
        check_input(schema, &json!({})).unwrap();
        assert!(schema.get("required").is_none(), "MCP accepts omitted keys for hook defaults");
        assert_eq!(s.execute("sample.make", &json!({"approval":approval,"params":{}})).unwrap()["result"]["width"], 24);
        assert_eq!(s.execute("plugin.run", &args(json!({}))).unwrap()["result"]["width"], 24);
        assert_eq!(s.execute("plugin.run", &args(json!({"width":12}))).unwrap()["result"]["width"], 12);
        let error = s.execute("plugin.run", &args(json!({"size":"small"}))).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::InvalidInput { field, .. }) if field == "params.size"), "{error}");
        assert!(s.undo.is_empty());
    }

    #[test]
    fn plugin_run_dialog_hook_timeout_is_typed_and_never_runs_the_entry() {
        let f = dialog_fixture("(loop $forever br $forever)");
        let path = f.0.join("sample/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["runtime"]["limits"]["deadline_ms"] = json!(1);
        std::fs::write(path, manifest.to_string()).unwrap();
        let mut s = Session::default();
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::Exhausted { limit, .. }) if limit == "deadline_ms"), "{error}");
        assert!(s.undo.is_empty());
    }

    #[test]
    fn plugin_run_omits_hidden_keys_and_refuses_unavailable_choices() {
        // Return the JSON argument received by the actual Wasm entry, using its canonical result area.
        let f = Fixture::new("i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let mut m = manifest();
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{
        "type":"object","required":["shelves"],"properties":{
            "storage":{"type":"string","enum":["shelves","drawers"]},
            "shelves":{"type":"integer","x-solidcraft-visible-when":{"field":"storage","eq":"shelves"}},
            "size":{"type":"string","oneOf":[{"const":"small","x-solidcraft-when":{"field":"storage","eq":"shelves"}},{"const":"large"}]}
        }}}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        let args = |params: Value| json!({"path":f.path(),"approval":approval,"entry":"make","params":params});
        let output = s.execute("plugin.run", &args(json!({"storage":"drawers","shelves":"ignored","size":"large"}))).unwrap();
        assert_eq!(output["result"], json!({"storage":"drawers","size":"large"}));
        let error = s.execute("plugin.run", &args(json!({"storage":"drawers","size":"small"}))).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::InvalidInput { field, .. }) if field == "params.size"));
        assert!(s.undo.is_empty(), "a read entry changes no design");
    }

    #[test]
    fn declared_entry_inputs_are_discoverable_and_namespaced_calls_share_approval() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        let schema = json!({"type":"object","properties":{"targets":{"type":"array","items":{"type":"string"}}}});
        m["runtime"]["entries"] = json!([{"name":"make","effect":"edit","input_schema":schema,"slots":[{"kind":"menu","name":"tools"}]}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let inspection = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let listed = s.execute("plugin.list", &json!({})).unwrap();
        let entry = listed["plugins"].as_array().unwrap().iter().find(|p| p["manifest"]["id"] == "sample").unwrap();
        assert_eq!(entry["commands"], json!(["sample.make"]));
        assert_eq!(entry["manifest"]["runtime"]["entries"][0]["input_schema"], schema);
        let description = s.execute("engine.describe", &json!({"command":"sample.make"})).unwrap();
        assert_eq!(description["input"]["properties"]["params"], craft_plugin::strict_input_schema(&schema), "as strict as the check");
        assert_eq!(description["tool"]["input"], description["input"]["properties"]["params"], "the tool takes the inputs alone");
        assert_eq!(description["effect"], "edit");
        assert!(s.doc.features.is_empty());
        assert!(s.execute("sample.make", &json!({"params":{}})).is_err());
        s.execute("sample.make", &json!({"approval":inspection["approval"],"params":{"targets":[]}})).unwrap();
        assert_eq!(s.undo.len(), 1);
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 24.);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.features.is_empty());
    }

    #[test]
    fn stale_plugin_worker_is_refused_after_edit_and_undo_restore_the_authored_snapshot() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let expected = s.design_token();
        let original = s.doc.clone();
        let bytes = s.doc.to_json();
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        let args = json!({"path":f.path(),"approval":approval,"entry":"make","expected_design":expected});
        let prepared = s.scratch().compute_commands(&[("plugin.run".into(), args)], None).unwrap();
        s.execute("parameters.add", &json!({"name":"width","expression":"10 mm"})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&s.doc, &original));
        assert_eq!(s.doc.to_json(), bytes);
        let actual = s.design_token();
        assert_ne!(actual, expected);
        assert!(!prepared.is_current(&s));
        let error = s.accept_commands(prepared).unwrap_err();
        assert_eq!(error.host_error(), Some(&HostError::StaleRevision { expected, actual }));
        assert_eq!(s.doc.to_json(), bytes);
    }

    #[test]
    fn stale_plugin_worker_result_is_refused_at_commit() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let expected = s.design_token();
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        let args = json!({"path":f.path(),"approval":approval,"entry":"make","expected_design":expected});
        let prepared = s.scratch().compute_commands(&[("plugin.run".into(), args)], None).unwrap();
        s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
        let actual = s.design_token();
        let before = s.doc.to_json();
        let history = s.undo.len();
        let error = s.accept_commands(prepared).unwrap_err();
        assert!(
            matches!(error.host_error(), Some(HostError::StaleRevision { expected: was, actual: now }) if was == &expected && now == &actual),
            "{error}"
        );
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), history);
    }

    #[test]
    fn listener_panel_cache_is_shared_and_bounded() {
        let mut s = Session::default();
        let oversized = Value::String("x".repeat(MAX_JSON_BYTES));
        assert!(matches!(events::listener_panel_data(&mut s, "sample", "oversized", oversized), Err(HostError::LimitExceeded { .. })));
        assert!(s.plugin_state.listener_data.is_empty());
        for index in 0..16 {
            let data = Value::String("x".repeat(MAX_JSON_BYTES - 128));
            events::listener_panel_data(&mut s, "sample", &format!("refresh{index}"), data).unwrap();
        }
        let scratch = s.scratch();
        assert!(std::sync::Arc::ptr_eq(&s.plugin_state.listener_data, &scratch.plugin_state.listener_data), "scratch copies share cache payloads");
        let error = events::listener_panel_data(&mut s, "sample", "extra", Value::String("x".repeat(4096))).unwrap_err();
        assert!(matches!(error, HostError::LimitExceeded { .. }), "{error}");
        assert_eq!(s.plugin_state.listener_data.len(), 16);
        assert!(
            std::sync::Arc::ptr_eq(&s.plugin_state.listener_data, &scratch.plugin_state.listener_data),
            "refusal does not copy or change the cache"
        );
    }

    #[test]
    fn listener_cache_updates_do_not_invalidate_computed_plugin_edits() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        let args = json!({"path":f.path(),"approval":approval,"entry":"make","expected_design":s.design_token()});
        let prepared = s.scratch().compute_commands(&[("plugin.run".into(), args)], None).unwrap();
        std::sync::Arc::make_mut(&mut s.plugin_state.listener_data).insert("sample.refresh".into(), json!({"status":"updated"}));
        s.plugin_state.listener_failures.insert("another".into(), "refused edit".into());
        s.accept_commands(prepared).unwrap();
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 24.);
        assert_eq!(s.plugin_state.listener_data["sample.refresh"], json!({"status":"updated"}));
    }

    #[test]
    fn stale_plugin_edits_name_both_tokens_and_leave_history_unchanged() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let expected = s.design_token();
        s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
        let actual = s.design_token();
        let before = s.doc.to_json();
        let history = (s.undo.len(), s.redo.len());
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        let args = json!({"path":f.path(),"approval":approval,"entry":"make","expected_design":expected});
        let error = s.execute("plugin.run", &args).unwrap_err();
        assert!(
            matches!(error.host_error(), Some(HostError::StaleRevision { expected: was, actual: now }) if was == &expected && now == &actual),
            "{error}"
        );
        assert_eq!(s.doc.to_json(), before);
        assert_eq!((s.undo.len(), s.redo.len()), history);
        let mut current = args;
        current["expected_design"] = json!(actual);
        s.execute("plugin.run", &current).unwrap();
        assert_ne!(s.design_token(), actual);
    }

    #[test]
    fn approved_design_listeners_receive_tokens_and_closed_design_without_editing() {
        let f = Fixture::new("i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.2", "design@0.1"]);
        m["runtime"]["commands"] = json!([]);
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
        m["runtime"]["listeners"] = json!([{"entry":"make","on":["design.changed"]}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        // Merely inspecting a package must never authorize a background invocation.
        s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let token = s.design_token();
        design_changed(&mut s, Some(token.clone()));
        assert!(s.plugin_state.listener_data.is_empty());
        f.install(&mut s, json!({}));
        design_changed(&mut s, Some(token.clone()));
        assert!(s.plugin_state.listener_failures.is_empty(), "{:?}", s.plugin_state.listener_failures);
        assert_eq!(s.plugin_state.listener_data["sample.make"], json!({"design":token}));
        assert_eq!(s.design_token(), token);
        design_changed(&mut s, None);
        assert_eq!(s.plugin_state.listener_data["sample.make"], json!({"design":null}));
        s.disabled_plugins.insert("sample".into());
        design_changed(&mut s, Some(token));
        assert_eq!(s.plugin_state.listener_data["sample.make"], json!({"design":null}));
    }

    #[test]
    fn linked_file_listeners_name_observed_and_read_sources_without_changing_legacy_payloads() {
        let f = Fixture::new("i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.2", "design@0.1"]);
        m["runtime"]["commands"] = json!([]);
        m["runtime"]["permissions"]["files"] = json!([{"name":"project","read":true,"write":false}]);
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
        m["runtime"]["listeners"] = json!([{"entry":"make","on":["design.links_changed"]}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let source = f.0.join("part.sc");
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
        part.execute("file.save", &json!({"path":source})).unwrap();
        let mut s = Session::default();
        f.install(&mut s, json!({"folders":[{"name":"project","path":f.0,"read":true,"write":false}]}));
        let inserted = s.execute("component.insert_design", &json!({"path":source,"name":"Bracket"})).unwrap();
        let component = inserted["component"].as_u64().unwrap();
        let token = s.design_token();
        let before = s.doc.to_json();
        let history = s.undo.len();
        part.execute("document.metadata.set", &json!({"key":"description","value":"New file contents"})).unwrap();
        part.execute("file.save", &json!({})).unwrap();
        let checked = s.scratch().compute_commands(&[("component.links".into(), json!({}))], None).unwrap();
        assert!(s.plugin_state.link_changes.is_empty(), "checks on a copy do not publish before acceptance");
        assert_eq!(s.accept_commands(checked).unwrap()[0].1["links"][0]["changed"], true);
        let observations = s.plugin_state.link_changes.clone();
        s.execute("component.links", &json!({})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&observations, &s.plugin_state.link_changes), "identical checks do not enqueue events");
        let links: Vec<_> = observations.values().cloned().collect();
        design_notifications(&mut s, Some(token.clone()), false, &links);
        let expected = json!({"design":token,"links":[{"component":component,"name":"Bracket","path":source,"read":false}]});
        assert_eq!(s.plugin_state.listener_data["sample.make"], expected);
        assert!(s.plugin_state.listener_failures.is_empty(), "{:?}", s.plugin_state.listener_failures);
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), history);
        assert_eq!(s.design_token(), token);

        // The actual worker acceptance path must retain the successful read observation.
        let prepared = s.scratch().compute_commands(&[("component.update_links".into(), json!({}))], None).unwrap();
        assert!(!s.plugin_state.link_changes[&component].read, "computing alone does not read into the live design");
        s.accept_commands(prepared).unwrap();
        assert!(s.plugin_state.link_changes[&component].read);
        let links: Vec<_> = s.plugin_state.link_changes.values().cloned().collect();
        let updated_token = s.design_token();
        design_notifications(&mut s, Some(updated_token.clone()), true, &links);
        let mut expected = expected;
        expected["design"] = json!(updated_token);
        expected["links"][0]["read"] = json!(true);
        assert_eq!(s.plugin_state.listener_data["sample.make"], expected);
        assert_eq!(s.execute("component.links", &json!({})).unwrap()["links"][0]["changed"], false);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.plugin_state.link_changes.is_empty(), "undo forgets observations of bytes the design no longer holds");
        let observations = s.plugin_state.link_changes.clone();
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&observations, &s.plugin_state.link_changes), "redo does not read a source file");
        let missing = f.0.join("missing.sc");
        assert!(s.execute("component.relink", &json!({"component":component,"path":missing})).is_err());
        assert!(std::sync::Arc::ptr_eq(&observations, &s.plugin_state.link_changes), "failed reads publish nothing");
        let replacement = f.0.join("replacement.sc");
        part.execute("file.save", &json!({"path":replacement})).unwrap();
        s.execute("component.relink", &json!({"component":component,"path":replacement})).unwrap();
        let links: Vec<_> = s.plugin_state.link_changes.values().cloned().collect();
        let updated_token = s.design_token();
        design_notifications(&mut s, Some(updated_token.clone()), true, &links);
        assert_eq!(
            s.plugin_state.listener_data["sample.make"],
            json!({"design":updated_token,"links":[{"component":component,"name":"Bracket","path":replacement,"read":true}]})
        );

        m["runtime"]["listeners"][0]["on"] = json!(["design.changed"]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        f.install(&mut s, json!({"folders":[{"name":"project","path":f.0,"read":true,"write":false}]}));
        s.plugin_state.listener_data = Default::default();
        design_notifications(&mut s, Some(updated_token.clone()), false, &links);
        assert!(s.plugin_state.listener_data.is_empty(), "old listeners skip disk-only observations");
        design_notifications(&mut s, Some(updated_token.clone()), true, &links);
        assert_eq!(s.plugin_state.listener_data["sample.make"], json!({"design":updated_token}));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.plugin_state.link_changes.is_empty(), "undoing relink cannot replay an observation of the replaced path");
    }

    #[test]
    fn unread_link_notifications_require_approved_read_folders_but_read_versions_do_not() {
        let f = Fixture::new("i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let inside = f.0.join("allowed");
        let outside = f.0.join("allowed-other");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let sources = [inside.join("part.sc"), outside.join("part.sc")];
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.2", "design@0.1"]);
        m["runtime"]["commands"] = json!([]);
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
        m["runtime"]["listeners"] = json!([{"entry":"make","on":["design.links_changed"]}]);
        m["runtime"]["permissions"]["files"] = json!([{"name":"project","read":true,"write":false}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        f.install(&mut s, json!({"folders":[{"name":"project","path":inside,"read":true,"write":false}]}));
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
        part.execute("file.save", &json!({"path":sources[0]})).unwrap();
        std::fs::copy(&sources[0], &sources[1]).unwrap();
        let components: Vec<_> = sources
            .iter()
            .zip(["Allowed", "Private"])
            .map(|(path, name)| s.execute("component.insert_design", &json!({"path":path,"name":name})).unwrap()["component"].as_u64().unwrap())
            .collect();
        part.execute("document.metadata.set", &json!({"key":"description","value":"Changed source bytes"})).unwrap();
        part.execute("file.save", &json!({})).unwrap();
        std::fs::copy(&sources[0], &sources[1]).unwrap();
        let checked = s.execute("component.links", &json!({})).unwrap();
        assert!(checked["links"].as_array().unwrap().iter().all(|link| link["changed"] == true));
        let token = s.design_token();
        let links: Vec<_> = s.plugin_state.link_changes.values().cloned().collect();
        design_notifications(&mut s, Some(token.clone()), false, &links);
        assert_eq!(
            s.plugin_state.listener_data["sample.make"],
            json!({"design":token,"links":[{"component":components[0],"name":"Allowed","path":sources[0],"read":false}]})
        );
        assert!(s.plugin_state.listener_failures.is_empty(), "{:?}", s.plugin_state.listener_failures);
        s.plugin_state.listener_data = Default::default();
        let hidden: Vec<_> = links.iter().filter(|link| link.component == components[1]).cloned().collect();
        design_notifications(&mut s, Some(token.clone()), false, &hidden);
        assert!(s.plugin_state.listener_data.is_empty(), "a links-only listener is not invoked when every unread source is outside its folders");
        // The session's own scope narrows the grant too: with the granted folder outside it, the
        // unread link inside that folder stays hidden.
        let session_scope = solidcraft_io::vfs::PathScope::new(vec![outside.clone()]);
        solidcraft_io::vfs::scoped(Some(session_scope), || design_notifications(&mut s, Some(token.clone()), false, &links));
        assert!(s.plugin_state.listener_data.is_empty(), "a granted folder outside the session's scope reveals nothing unread");
        #[cfg(unix)]
        {
            let alias = inside.join("outside.sc");
            std::os::unix::fs::symlink(&sources[1], &alias).unwrap();
            let mut escaped = hidden[0].clone();
            escaped.path = alias.to_string_lossy().into_owned();
            design_notifications(&mut s, Some(token), false, &[escaped]);
            assert!(s.plugin_state.listener_data.is_empty(), "a symlink inside the approved folder cannot reveal an outside source");
        }

        s.execute("component.update_links", &json!({})).unwrap();
        let token = s.design_token();
        let links: Vec<_> = s.plugin_state.link_changes.values().cloned().collect();
        design_notifications(&mut s, Some(token.clone()), true, &links);
        assert_eq!(
            s.plugin_state.listener_data["sample.make"],
            json!({"design":token,"links":[
                {"component":components[0],"name":"Allowed","path":sources[0],"read":true},
                {"component":components[1],"name":"Private","path":sources[1],"read":true}
            ]})
        );
    }

    #[test]
    fn unread_link_notifications_skip_hidden_folders_inside_a_granted_folder() {
        let f = Fixture::new("i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let grant = f.0.join("grant");
        let sources = [grant.join(".hidden/part.step"), grant.join("visible/part.step")];
        for source in &sources {
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(source, "part").unwrap();
        }
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.2", "design@0.1"]);
        m["runtime"]["commands"] = json!([]);
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
        m["runtime"]["listeners"] = json!([{"entry":"make","on":["design.links_changed"]}]);
        m["runtime"]["permissions"]["files"] = json!([{"name":"project","read":true,"write":false}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        f.install(&mut s, json!({"folders":[{"name":"project","path":grant,"read":true,"write":false}]}));
        let links: Vec<_> = sources
            .iter()
            .zip([1, 2])
            .map(|(path, component)| LinkChange {
                component,
                name: None,
                path: path.to_string_lossy().into_owned(),
                read: false,
                digest: String::new(),
            })
            .collect();
        let token = s.design_token();
        design_notifications(&mut s, Some(token.clone()), false, &links[..1]);
        assert!(s.plugin_state.listener_data.is_empty(), "an unread source in a hidden folder is not reported");
        design_notifications(&mut s, Some(token.clone()), false, &links);
        assert_eq!(
            s.plugin_state.listener_data["sample.make"],
            json!({"design":token,"links":[{"component":2,"name":null,"path":sources[1],"read":false}]})
        );
        assert!(s.plugin_state.listener_failures.is_empty(), "{:?}", s.plugin_state.listener_failures);
    }

    #[test]
    fn prepared_link_checks_preserve_newer_live_observations() {
        let f = Fixture::new("");
        let source = f.0.join("part.sc");
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
        part.execute("file.save", &json!({"path":source})).unwrap();
        let original = std::fs::read(&source).unwrap();
        let mut s = Session::default();
        let component = s.execute("component.insert_design", &json!({"path":source})).unwrap()["component"].as_u64().unwrap();
        part.execute("document.metadata.set", &json!({"key":"description","value":"Changed source bytes"})).unwrap();
        part.execute("file.save", &json!({})).unwrap();
        let newer = std::fs::read(&source).unwrap();
        let prepared = s.scratch().compute_commands(&[("component.links".into(), json!({}))], None).unwrap();
        s.execute("component.links", &json!({})).unwrap();
        assert!(!s.plugin_state.link_changes[&component].read);
        std::fs::write(&source, &original).unwrap();
        s.execute("component.links", &json!({})).unwrap();
        assert!(s.plugin_state.link_changes[&component].read);
        let live = s.plugin_state.link_changes.clone();
        let token = s.design_token();
        s.accept_commands(prepared).unwrap();
        assert!(std::sync::Arc::ptr_eq(&live, &s.plugin_state.link_changes), "a stale check cannot overwrite the live observation");
        assert_eq!(s.design_token(), token);

        let inherited = s.scratch().compute_commands(&[("component.links".into(), json!({}))], None).unwrap();
        std::fs::write(&source, &newer).unwrap();
        s.execute("component.links", &json!({})).unwrap();
        let live = s.plugin_state.link_changes.clone();
        s.accept_commands(inherited).unwrap();
        assert!(std::sync::Arc::ptr_eq(&live, &s.plugin_state.link_changes), "inherited read flags are not new source reads");

        std::fs::write(&source, &original).unwrap();
        s.execute("component.links", &json!({})).unwrap();
        std::fs::write(&source, &newer).unwrap();
        let prepared = s.scratch().compute_commands(&[("component.links".into(), json!({}))], None).unwrap();
        s.accept_commands(prepared).unwrap();
        assert!(!s.plugin_state.link_changes[&component].read, "without an intervening observation acceptance still publishes the check");

        std::fs::write(&source, &original).unwrap();
        let held = s.scratch().compute_commands(&[("component.links".into(), json!({}))], None).unwrap();
        part.execute("document.metadata.set", &json!({"key":"description","value":"A third, newer observation of the source"})).unwrap();
        part.execute("file.save", &json!({"overwrite":true})).unwrap();
        s.execute("component.links", &json!({})).unwrap();
        let live = s.plugin_state.link_changes.clone();
        s.accept_commands(held).unwrap();
        assert!(std::sync::Arc::ptr_eq(&live, &s.plugin_state.link_changes), "a check's read:true also obeys the source observation guard");
    }

    #[test]
    fn unchanged_link_reads_supersede_stale_observations_only_for_successful_selected_links() {
        let f = Fixture::new("");
        let sources = [f.0.join("good.sc"), f.0.join("failed.sc"), f.0.join("unselected.sc")];
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
        part.execute("file.save", &json!({"path":sources[0]})).unwrap();
        let original = std::fs::read(&sources[0]).unwrap();
        let mut s = Session::default();
        let components: Vec<_> = sources
            .iter()
            .map(|path| {
                std::fs::write(path, &original).unwrap();
                s.execute("component.insert_design", &json!({"path":path})).unwrap()["component"].as_u64().unwrap()
            })
            .collect();
        let digest = s.doc.links[&components[0]].file_sha256.clone();
        part.execute("document.metadata.set", &json!({"key":"description","value":"Changed source bytes"})).unwrap();
        part.execute("file.save", &json!({"overwrite":true})).unwrap();
        let newer = std::fs::read(&sources[0]).unwrap();
        for source in &sources {
            std::fs::write(source, &newer).unwrap();
        }
        s.execute("component.links", &json!({})).unwrap();
        assert!(components.iter().all(|component| !s.plugin_state.link_changes[component].read));
        std::fs::write(&sources[0], &original).unwrap();
        std::fs::write(&sources[1], b"invalid design").unwrap();
        let params = json!({"components":[components[0],components[1]]});
        let reply = s.execute("component.update_links", &params).unwrap();
        assert_eq!(reply["failed"][0]["component"], components[1]);
        assert_eq!(s.doc.links[&components[0]].file_sha256, digest);
        assert!(s.plugin_state.link_changes[&components[0]].read, "reading the held version clears the older disk observation");
        assert!(!s.plugin_state.link_changes[&components[1]].read, "failed links are not marked read");
        assert!(!s.plugin_state.link_changes[&components[2]].read, "unselected links are not marked read");

        // The same no-op source read must survive the worker's acceptance path too.
        std::fs::write(&sources[0], &newer).unwrap();
        s.execute("component.links", &json!({})).unwrap();
        std::fs::write(&sources[0], &original).unwrap();
        let prepared = s.scratch().compute_commands(&[("component.update_links".into(), params.clone())], None).unwrap();
        assert!(!s.plugin_state.link_changes[&components[0]].read);
        s.accept_commands(prepared).unwrap();
        assert_eq!(s.doc.links[&components[0]].file_sha256, digest);
        assert!(s.plugin_state.link_changes[&components[0]].read);
        assert!(!s.plugin_state.link_changes[&components[1]].read);
        assert!(!s.plugin_state.link_changes[&components[2]].read);

        // Plugin transactions accept evaluated sessions directly, sometimes in two layers.
        std::fs::write(&sources[0], &newer).unwrap();
        s.execute("component.links", &json!({})).unwrap();
        std::fs::write(&sources[0], &original).unwrap();
        let mut transaction = s.scratch();
        transaction.execute("component.update_links", &params).unwrap();
        let mut plugin_call = s.scratch();
        plugin_call.accept_computed(transaction).unwrap();
        s.accept_computed(plugin_call).unwrap();
        assert!(s.plugin_state.link_changes[&components[0]].read, "no-op reads survive nested transaction acceptance");
        assert!(!s.plugin_state.link_changes[&components[1]].read);
        assert!(!s.plugin_state.link_changes[&components[2]].read);
    }

    #[test]
    fn computed_noop_link_reads_preserve_newer_disk_observations() {
        fn accept(s: &mut Session, prepared: crate::ComputedCommands, commands: bool) {
            if commands {
                s.accept_commands(prepared).unwrap();
            } else {
                s.accept_computed(prepared.computed).unwrap();
            }
        }

        for commands in [false, true] {
            let f = Fixture::new("");
            let source = f.0.join("part.sc");
            let mut part = Session::default();
            part.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
            part.execute("file.save", &json!({"path":source})).unwrap();
            let original = std::fs::read(&source).unwrap();
            let mut s = Session::default();
            let component = s.execute("component.insert_design", &json!({"path":source})).unwrap()["component"].as_u64().unwrap();
            let held = s.doc.links[&component].file_sha256.clone();
            part.execute("document.metadata.set", &json!({"key":"description","value":"Changed source bytes"})).unwrap();
            part.execute("file.save", &json!({})).unwrap();
            let newer = std::fs::read(&source).unwrap();
            let update = [("component.update_links".into(), json!({"component":component}))];

            // The worker read A, then a live check observed B before that no-op was accepted.
            std::fs::write(&source, &original).unwrap();
            let prepared = s.scratch().compute_commands(&update, None).unwrap();
            std::fs::write(&source, &newer).unwrap();
            s.execute("component.links", &json!({})).unwrap();
            let observed = s.plugin_state.link_changes.clone();
            assert!(!observed[&component].read);
            accept(&mut s, prepared, commands);
            assert_eq!(s.doc.links[&component].file_sha256, held);
            assert!(std::sync::Arc::ptr_eq(&observed, &s.plugin_state.link_changes), "a no-op read cannot overwrite a newer disk check");

            // Without an intervening observation, the same no-op read is still published.
            std::fs::write(&source, &original).unwrap();
            let prepared = s.scratch().compute_commands(&update, None).unwrap();
            accept(&mut s, prepared, commands);
            assert!(s.plugin_state.link_changes[&component].read);
            assert_eq!(Some(&s.plugin_state.link_changes[&component].digest), held.as_ref());

            // Applying changed source bytes supersedes a check even when that check is newer.
            std::fs::write(&source, &newer).unwrap();
            let prepared = s.scratch().compute_commands(&update, None).unwrap();
            s.execute("component.links", &json!({})).unwrap();
            assert!(!s.plugin_state.link_changes[&component].read);
            accept(&mut s, prepared, commands);
            assert_ne!(s.doc.links[&component].file_sha256, held);
            assert!(s.plugin_state.link_changes[&component].read);
            assert_eq!(s.doc.links[&component].file_sha256.as_ref(), Some(&s.plugin_state.link_changes[&component].digest));
        }
    }

    #[test]
    fn listener_success_clears_only_that_entrys_failure() {
        let f = Fixture::new(
            "local.get 0 i32.load8_u i32.const 102 i32.eq if unreachable end
             i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store",
        );
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.2", "design@0.1"]);
        m["runtime"]["commands"] = json!([]);
        m["runtime"]["entries"] = json!([
            {"name":"fail","effect":"read","input_schema":{"type":"object"}},
            {"name":"refresh","effect":"read","input_schema":{"type":"object"}}
        ]);
        m["runtime"]["listeners"] = json!([
            {"entry":"fail","on":["design.changed"]}, {"entry":"refresh","on":["design.links_changed"]}
        ]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        f.install(&mut s, json!({}));
        let token = s.design_token();
        design_changed(&mut s, Some(token.clone()));
        let failure = s.plugin_state.listener_entry_failures["sample"]["fail"].clone();
        let diagnostic = s.plugin_state.listener_failures["sample"].clone();
        let links = [LinkChange { component: 1, name: Some("Bracket".into()), path: "part.sc".into(), read: true, digest: "new".into() }];
        design_notifications(&mut s, Some(token.clone()), false, &links);
        assert!(s.plugin_state.listener_data.contains_key("sample.refresh"), "the other listener succeeded");
        assert_eq!(s.plugin_state.listener_entry_failures["sample"]["fail"], failure);
        assert_eq!(s.plugin_state.listener_failures["sample"], diagnostic, "links-only success retains the design listener's diagnostic");
        design_notifications(&mut s, Some(token.clone()), true, &links);
        assert_eq!(s.plugin_state.listener_entry_failures["sample"]["fail"], failure, "later success in the same delivery cannot clear it");

        std::fs::write(f.0.join("sample/plugin.wasm"), component("i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store")).unwrap();
        f.install(&mut s, json!({}));
        design_changed(&mut s, Some(token));
        assert!(!s.plugin_state.listener_entry_failures.contains_key("sample"), "that entry's own success clears its failure");
        assert!(!s.plugin_state.listener_failures.contains_key("sample"));
    }

    #[test]
    fn design_listeners_refuse_point_cloud_work_before_it_starts() {
        let mut session = Session::default();
        session.plugin_state.listener_design = Some(Some(session.design_token()));
        let mut host = SolidCraftHost {
            forms: Vec::new(),
            form_source: None,
            session,
            transaction: None,
            sketch_exits: Vec::new(),
            view: None,
            groups: BTreeSet::new(),
            file_folders: Vec::new(),
            file_read_folders: Vec::new(),
            evaluation: None,
        };
        assert_eq!(catalog_command("pointcloud.deviation").unwrap().effect, Effect::Read);
        host.begin_transaction().unwrap();
        let params = json!({"cloud":"scan","body":"part"});
        let error = host.run_command("pointcloud.deviation", &params).unwrap_err();
        assert!(matches!(error, HostError::PermissionDenied { permission }
            if permission.contains("pointcloud.deviation") && permission.contains("read entry")));
        // An explicit user invocation retains the existing read-only command contract.
        host.session.plugin_state.listener_design = None;
        assert!(matches!(host.run_command("pointcloud.deviation", &params), Err(HostError::CommandFailed { .. })));
        host.rollback_transaction().unwrap();
    }

    #[test]
    fn design_listeners_are_read_only_and_never_fail_the_triggering_action() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.2", "design@0.1"]);
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
        m["runtime"]["listeners"] = json!([{"entry":"make","on":["design.changed"]}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        f.install(&mut s, json!({}));
        s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
        let token = s.design_token();
        let history = s.undo.len();
        design_changed(&mut s, Some(token.clone()));
        assert_eq!(s.design_token(), token);
        assert_eq!(s.undo.len(), history);
        assert!(s.plugin_state.listener_failures["sample"].contains("read entry"));
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 1.);
        // A guest can return a long typed diagnostic. Keep its actionable start without
        // retaining or laying out the guest's whole string on the window thread.
        let long_error = Fixture::new(
            "i32.const 0 i32.const 1 i32.store i32.const 4 i32.const 5 i32.store
             i32.const 8 i32.const 8192 i32.store i32.const 12 i32.const 5000 i32.store
             i32.const 8192 i32.const 120 i32.const 5000 memory.fill",
        );
        std::fs::write(long_error.0.join("sample/plugin.json"), m.to_string()).unwrap();
        long_error.install(&mut s, json!({}));
        design_changed(&mut s, Some(token.clone()));
        let failure = &s.plugin_state.listener_failures["sample"];
        assert_eq!(failure.chars().count(), 4096);
        assert!(failure.contains(&"x".repeat(32)), "the guest diagnostic was retained");
        assert_eq!(s.design_token(), token);
        assert_eq!(s.undo.len(), history);
    }

    #[test]
    fn read_entries_cannot_edit_even_with_an_edit_command_grant() {
        for command in ["solid.box", "sketch.edit"] {
            let f = Fixture::new("call $box");
            if command == "sketch.edit" {
                let source = component_template()
                    .replace("      call $box\n      call $box\n", "      call $box\n")
                    .replace("solid.box", "sketch.edit")
                    .replace(r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#, r#"{\22sketch\22:\22Sketch1\22}"#)
                    .replace("i32.const 9\n", "i32.const 11\n")
                    .replace("i32.const 33\n", "i32.const 20\n");
                std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(source).unwrap()).unwrap();
            }
            let mut m = manifest();
            m["runtime"]["commands"] = json!([command]);
            m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
            std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
            let mut s = Session::default();
            s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
            s.execute("sketch.finish", &json!({})).unwrap();
            s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
            let before = s.doc.to_json();
            let history = (s.undo.len(), s.redo.len());
            let error = f.run(&mut s).unwrap_err();
            assert!(
                matches!(error.host_error(), Some(craft_plugin::HostError::PermissionDenied { permission }) if permission.contains("read entry")),
                "{command}: {error}"
            );
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(s.active_sketch, None);
            assert_eq!((s.undo.len(), s.redo.len()), history);
        }
    }

    #[test]
    fn accepted_plugin_replies_identify_inner_sketch_exits() {
        for command in ["sketch.finish", "sketch.cancel"] {
            for trap in [false, true] {
                let f = Fixture::new("call $box");
                let source = component_template()
                    .replace(
                        "      call $box\n      call $box\n",
                        if trap {
                            "      i32.const 2048 i32.const 11 i32.const 3072 i32.const 20 i32.const 0 call $run call $box unreachable\n"
                        } else {
                            "      i32.const 2048 i32.const 11 i32.const 3072 i32.const 20 i32.const 0 call $run call $box\n"
                        },
                    )
                    .replace("solid.box", command)
                    .replace(
                        "file.export",
                        r#"sketch.edit")
    (data (i32.const 3072) "{\22sketch\22:\22Sketch1\22}"#,
                    )
                    .replace(r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#, "{}")
                    .replace("i32.const 9\n", &format!("i32.const {}\n", command.len()))
                    .replace("i32.const 33\n", "i32.const 2\n");
                std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(source).unwrap()).unwrap();
                let mut m = manifest();
                m["runtime"]["commands"] = json!(["sketch.edit", command]);
                std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
                let mut s = Session::default();
                s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
                let open = s.active_sketch.unwrap();
                let before = s.doc.to_json();
                let result = f.run(&mut s);
                if trap {
                    assert!(result.is_err());
                    assert_eq!(s.active_sketch, Some(open));
                    assert_eq!(s.doc.to_json(), before);
                } else {
                    assert_eq!(result.unwrap()["sketch_exits"], json!([open]));
                    assert_eq!(s.active_sketch, None);
                }
            }
        }
    }

    #[test]
    fn folder_grants_apply_the_caller_scope_to_each_hidden_read_and_write() {
        for write in [false, true] {
            for name in [".env", ".hidden/secret.txt"] {
                let f = Fixture::new("call $box");
                std::fs::create_dir_all(f.0.join("sample/.hidden")).unwrap();
                let target = f.0.join("sample").join(name);
                std::fs::write(&target, b"private contents").unwrap();
                std::fs::write(f.0.join("sample/plugin.wasm"), file_component_path(write, name)).unwrap();
                let mut m = manifest();
                m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":true}]});
                std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
                let mut s = Session::default();
                s.plugin_state.file_grants.push(FileGrant { name: "project".into(), folder: f.0.join("sample"), read: true, write: true });
                s.plugin_state.grants.insert(
                    "sample".into(),
                    Grants {
                        package_hash: Some(read_package(&f.path()).unwrap().approval),
                        command_groups: BTreeSet::new(),
                        folders: s.plugin_state.file_grants.clone(),
                        fuel: None,
                        memory_mib: None,
                        input_folder: false,
                    },
                );
                let scope = solidcraft_io::vfs::PathScope::default().with_design_folder(&f.0);
                assert!(scope.check(&f.path()).is_ok());
                for scope in [None, Some(scope)] {
                    solidcraft_io::vfs::scoped(scope, || {
                        let error = f.run(&mut s).unwrap_err();
                        assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { .. })), "{write} {name}: {error}");
                    });
                    assert_eq!(std::fs::read(&target).unwrap(), b"private contents");
                }
                let host = SolidCraftHost {
                    session: s,
                    transaction: None,
                    sketch_exits: Vec::new(),
                    view: None,
                    groups: BTreeSet::new(),
                    file_folders: Vec::new(),
                    file_read_folders: Vec::new(),
                    evaluation: None,
                    forms: Vec::new(),
                    form_source: None,
                };
                solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([f.0.clone()])), || {
                    assert!(host.authorize_file_path(&target, write).is_ok(), "explicit scope grants hidden access");
                });
            }
        }
    }

    #[test]
    fn hand_installed_plugins_get_no_legacy_or_other_plugins_folder_grants() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("sample/plugin.wasm"), file_component(false)).unwrap();
        std::fs::write(f.0.join("sample/secret.txt"), "test data").unwrap();
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":false}]});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let grant = FileGrant { name: "project".into(), folder: f.0.join("sample"), read: true, write: false };
        s.plugin_state.file_grants.push(grant.clone());
        s.plugin_state.grants.insert("another-plugin".into(), Grants { folders: vec![grant], ..Default::default() });
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { .. })), "{error}");
        assert!(s.doc.features.is_empty());
    }

    #[test]
    fn only_the_running_plugins_stored_command_groups_authorize_sensitive_calls() {
        let f = Fixture::new("call $box");
        let command = "settings.profile.list";
        let source = component_template();
        let arguments = r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#;
        assert!(source.contains(arguments));
        let source = source
            .replace("\"solid.box\"", &format!("\"{command}\""))
            .replace(
                "i32.const 9\n      i32.const 1024\n      i32.const 33",
                &format!("i32.const {}\n      i32.const 1024\n      i32.const 2", command.len()),
            )
            .replace(arguments, "{}");
        std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(&source).unwrap()).unwrap();
        let mut m = manifest();
        m["runtime"]["commands"] = json!([command]);
        m["runtime"]["permissions"] = json!({"command_groups":["app"]});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        s.plugin_state.command_groups.insert("app".into());
        s.plugin_state.grants.insert("another-plugin".into(), Grants { command_groups: BTreeSet::from(["app".into()]), ..Default::default() });
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { permission }) if permission.contains("app")), "{error}");
        let bytes =
            solidcraft_io::safe_zip::write(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", &wat::parse_str(&source).unwrap())])
                .unwrap();
        let zip = f.0.join("package.zip");
        std::fs::write(&zip, bytes).unwrap();
        s.plugin_state.plugins_dir = Some(f.0.join("installed"));
        s.execute("plugin.install", &json!({"path":zip,"approve":{"command_groups":["app"]}})).unwrap();
        assert_eq!(f.run(&mut s).unwrap()["result"], json!({}));
        s.plugin_state.grants.remove("sample");
        assert!(matches!(f.run(&mut s).err().as_ref().and_then(EngineError::host_error), Some(HostError::PermissionDenied { .. })));
        assert!(s.doc.features.is_empty());
    }

    /// `plugin.check` finds what Install and a call would refuse, with a fix for each, and runs
    /// no plugin code: the fixture's component would trap if it ran.
    #[test]
    fn check_reports_each_refusal_with_its_fix_and_runs_nothing() {
        let f = Fixture::new("unreachable");
        let mut s = Session::default();
        let check = |s: &mut Session, path: &str| s.execute("plugin.check", &json!({"path":path})).unwrap();
        let good = check(&mut s, &f.path());
        assert_eq!((good["ok"].as_bool(), good["kind"].as_str()), (Some(true), Some("command")), "{good}");
        assert_eq!(good["problems"], json!([]));
        assert_eq!(good["approval"], s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"], "one hash for one package");
        // The package as it ships: a .zip, checked by the installer's own reader.
        let component = std::fs::read(f.0.join("sample/plugin.wasm")).unwrap();
        let zip = |manifest: &Value, component: &[u8]| {
            let path = f.0.join("package.zip");
            let manifest = serde_json::to_vec(manifest).unwrap();
            std::fs::write(&path, solidcraft_io::safe_zip::write(&[("plugin.json", &manifest), ("plugin.wasm", component)]).unwrap()).unwrap();
            path.to_string_lossy().into_owned()
        };
        let zipped = check(&mut s, &zip(&manifest(), &component));
        assert_eq!((zipped["ok"].as_bool(), &zipped["approval"]), (Some(true), &good["approval"]));
        // Hidden files and the tests folder stay out of a package folder's check.
        std::fs::create_dir_all(f.0.join("sample/tests")).unwrap();
        std::fs::write(f.0.join("sample/tests/big.bin"), vec![0u8; 17 << 20]).unwrap();
        std::fs::write(f.0.join("sample/.notes"), b"x").unwrap();
        assert_eq!(check(&mut s, &f.path())["problems"], json!([]));
        // A name the .zip reader refuses is refused in a folder too (Windows cannot even make it).
        #[cfg(unix)]
        {
            std::fs::write(f.0.join("sample/notes\\old.txt"), b"x").unwrap();
            let reply = check(&mut s, &f.path());
            assert_eq!((reply["ok"].as_bool(), reply["problems"][0]["field"].as_str()), (Some(false), Some("package")), "{reply}");
            assert!(reply["problems"][0]["problem"].as_str().unwrap().contains("backslash"), "{reply}");
            std::fs::remove_file(f.0.join("sample/notes\\old.txt")).unwrap();
            std::fs::create_dir_all(f.0.join("sample/notes")).unwrap();
            std::fs::write(f.0.join("sample/notes/old.txt"), b"x").unwrap();
            assert_eq!(check(&mut s, &f.path())["problems"], json!([]), "a plain path in a folder is fine");
            std::fs::remove_dir_all(f.0.join("sample/notes")).unwrap();
        }
        let fields = |reply: &Value| -> Vec<(String, String)> {
            reply["problems"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p["severity"].as_str().unwrap().into(), p["field"].as_str().unwrap().into()))
                .collect()
        };
        for (mutate, field) in [
            ((|m: &mut Value| m["runtime"]["permissions"] = json!({"net": true})) as fn(&mut Value), "runtime.permissions.net"),
            (|m: &mut Value| m["id"] = json!("Sample"), "id"),
            (|m: &mut Value| m["capabilities"] = json!(["commands@0.3"]), "capabilities"),
            (|m: &mut Value| m["runtime"]["commands"] = json!(["file.export"]), "runtime.commands"),
            (|m: &mut Value| m["runtime"]["limits"]["deadline_ms"] = json!(60_000), "runtime.limits.deadline_ms"),
            (|m: &mut Value| m["runtime"]["limits"]["fuel"] = json!(10_000_000_000_001_u64), "runtime.limits.fuel"),
            (|m: &mut Value| m["runtime"]["module"] = json!("missing.wasm"), "runtime.module"),
            (|m: &mut Value| m["surprise"] = json!(true), "manifest"),
        ] {
            let mut m = manifest();
            mutate(&mut m);
            let reply = check(&mut s, &zip(&m, &component));
            assert_eq!(reply["ok"], false, "{m}");
            assert_eq!(fields(&reply).first(), Some(&("error".to_string(), field.to_string())), "{reply}");
            assert!(reply["problems"].as_array().unwrap().iter().all(|p| !p["fix"].as_str().unwrap().is_empty()), "{reply}");
        }
        // Memory over the ceiling installs once the user approves it: a warning.
        let mut m = manifest();
        m["runtime"]["limits"]["memory_mib"] = json!(512);
        let reply = check(&mut s, &zip(&m, &component));
        assert_eq!((reply["ok"].as_bool(), fields(&reply)), (Some(true), vec![("warning".to_string(), "runtime.limits.memory_mib".to_string())]));
        m["runtime"]["limits"] = json!({"fuel":10_000_000_000_000_u64,"memory_mib":64,"deadline_ms":5000});
        let reply = check(&mut s, &zip(&m, &component));
        assert_eq!((reply["ok"].as_bool(), fields(&reply)), (Some(true), vec![("warning".to_string(), "runtime.limits.fuel".to_string())]));
        // A core module, and a command component declared as an importer, do not link.
        let core = wat::parse_str("(module)").unwrap();
        let reply = check(&mut s, &zip(&manifest(), &core));
        assert!(reply["problems"][0]["fix"].as_str().unwrap().contains("wasm-tools component new"), "{reply}");
        let importer = json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.1"],"runtime":{"module":"plugin.wasm",
            "commands":[],"entries":[{"name":"convert","kind":"importer","extensions":["smp"]}]}});
        assert_eq!(fields(&check(&mut s, &zip(&importer, &component))), vec![("error".to_string(), "component".to_string())]);
        // A folder named apart from its id still checks, with a note.
        std::fs::rename(f.0.join("sample"), f.0.join("sample-src")).unwrap();
        let renamed = check(&mut s, &f.0.join("sample-src").to_string_lossy());
        assert_eq!((renamed["ok"].as_bool(), fields(&renamed)), (Some(true), vec![("warning".to_string(), "id".to_string())]));
        std::fs::rename(f.0.join("sample-src"), f.0.join("sample")).unwrap();
        assert!(s.doc.features.is_empty() && s.undo.is_empty(), "checking changes nothing");
        assert!(!f.0.join("installed").exists(), "and installs nothing");
    }

    #[test]
    fn installed_plugins_run_with_only_the_approved_folders() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("secret.txt"), "{}").unwrap();
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":false}]});
        let bytes =
            solidcraft_io::safe_zip::write(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", &file_component(false))]).unwrap();
        let zip = f.0.join("package.zip");
        std::fs::write(&zip, bytes).unwrap();
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(f.0.join("installed"));
        let installed = s.execute("plugin.install", &json!({"path":zip})).unwrap();
        let path = installed["folder"].clone();
        let approval = s.execute("plugin.inspect", &json!({"path":path})).unwrap()["approval"].clone();
        let params = json!({"path":path,"approval":approval,"entry":"make"});
        assert!(matches!(
            s.execute("plugin.run", &params).err().as_ref().and_then(EngineError::host_error),
            Some(HostError::PermissionDenied { .. })
        ));
        s.execute(
            "plugin.install",
            &json!({"path":zip,"replace":true,"approve":{"folders":[{"name":"project","path":f.0,"read":true,"write":false}]}}),
        )
        .unwrap();
        assert_eq!(s.plugin_state.grants["sample"].folders[0].folder, f.0);
        assert!(s.execute("plugin.run", &params).is_ok());
        s.execute("plugin.remove", &json!({"id":"sample"})).unwrap();
        assert!(!s.plugin_state.grants.contains_key("sample"));
        assert!(s.execute("plugin.run", &params).is_err());
    }

    #[test]
    fn stored_grants_refuse_same_id_packages_with_different_manifest_or_component() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"command_groups":["app"],"files":[{"name":"project","read":true,"write":true}]});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let installed = f.install(&mut s, json!({"command_groups":["app"],"folders":[{"name":"project","path":f.0,"read":true,"write":true}]}));
        let trusted = read_package(&installed).unwrap();
        assert_eq!(s.plugin_state.grants["sample"].package_hash.as_deref(), Some(trusted.approval.as_str()));
        let other = Fixture::new("call $box");
        m["version"] = json!("0.2.0");
        std::fs::write(other.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let grants = s.plugin_state.grants.clone();
        assert!(grants["sample"].command_groups.contains("app"));
        assert_eq!(grants["sample"].folders.len(), 1);
        for change_component in [false, true] {
            if change_component {
                std::fs::copy(f.0.join("sample/plugin.json"), other.0.join("sample/plugin.json")).unwrap();
                std::fs::write(other.0.join("sample/plugin.wasm"), component("call $box call $box")).unwrap();
            }
            let error = other.run(&mut s).unwrap_err();
            assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { permission }) if permission.contains("re-approval")), "{error}");
            assert!(s.doc.features.is_empty(), "refuse before even non-sensitive plugin edits");
            assert!(s.plugin_state.file_grants.is_empty() && s.plugin_state.command_groups.is_empty());
            assert_eq!(s.plugin_state.grants, grants, "inspection cannot change approved grants");
        }
        s.execute("plugin.run", &json!({"path":installed,"approval":trusted.approval,"entry":"make"})).unwrap();
        assert_eq!(s.doc.features.len(), 1, "the installed package still runs");
    }

    #[test]
    fn stored_grants_require_reapproval_after_installed_package_changes() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let installed = f.install(&mut s, json!({}));
        let old_hash = s.plugin_state.grants["sample"].package_hash.clone();
        let mut m = manifest();
        m["version"] = json!("0.2.0");
        for path in [f.0.join("sample/plugin.json"), Path::new(&installed).join("plugin.json")] {
            std::fs::write(path, m.to_string()).unwrap();
        }
        let inspected = s.execute("plugin.inspect", &json!({"path":installed})).unwrap();
        let args = json!({"path":installed,"approval":inspected["approval"],"entry":"make"});
        let error = s.execute("plugin.run", &args).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { permission }) if permission.contains("re-approval")), "{error}");
        assert!(s.doc.features.is_empty());
        assert_eq!(s.plugin_state.grants["sample"].package_hash, old_hash);
        f.install(&mut s, json!({}));
        assert_ne!(s.plugin_state.grants["sample"].package_hash, old_hash);
        s.execute("plugin.run", &args).unwrap();
        assert_eq!(s.doc.features.len(), 1);
    }

    #[test]
    fn legacy_stored_grants_require_reapproval_instead_of_trusting_an_inspection() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let legacy: Grants = serde_json::from_value(json!({"command_groups":["app"],"folders":[]})).unwrap();
        assert_eq!(legacy.package_hash, None);
        s.plugin_state.grants.insert("sample".into(), legacy.clone());
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { permission }) if permission.contains("re-approval")), "{error}");
        assert_eq!(s.plugin_state.grants["sample"], legacy);
        assert!(s.doc.features.is_empty() && s.plugin_state.file_grants.is_empty());
        f.install(&mut s, json!({}));
        f.run(&mut s).unwrap();
        assert!(s.plugin_state.grants["sample"].command_groups.is_empty());
        assert_eq!(s.doc.features.len(), 1);
    }

    #[test]
    fn omitted_entry_params_are_objects_in_both_call_forms() {
        let f = Fixture::new("i32.const 0 i32.const 0 i32.store i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let mut s = Session::default();
        let inspected = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let direct = s.execute("plugin.run", &json!({"path":f.path(),"approval":inspected["approval"],"entry":"make"})).unwrap();
        let namespaced = s.execute("sample.make", &json!({"approval":inspected["approval"]})).unwrap();
        assert_eq!(direct["result"], json!({}));
        assert_eq!(direct["result"], namespaced["result"]);
    }

    /// A component whose entry runs `storage.set` with `{"document":{"teeth":24}}`.
    fn storage_component() -> Vec<u8> {
        let template = component_template();
        let code = "      call $box\n      call $box\n";
        let data = "    (data (;2;) (i32.const 2048) \"file.export\")\n";
        assert!(template.contains(code) && template.contains(data));
        let source = template.replacen(code, "      i32.const 2560 i32.const 11 i32.const 3072 i32.const 25 i32.const 0 call $run\n", 1).replacen(
            data,
            &format!(
                "{data}    (data (i32.const 2560) \"storage.set\")\n    (data (i32.const 3072) \"{{\\22document\\22:{{\\22teeth\\22:24}}}}\")\n"
            ),
            1,
        );
        wat::parse_str(source).unwrap()
    }
    #[test]
    fn expected_design_and_form_guard_both_apply_to_the_same_plugin_call() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let expected = s.design_token();
        let revision = s.design_revision();
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        let mut args = json!({"path":f.path(),"approval":approval,"entry":"make","expected_design":expected,
            "form_guard":{"document_id":"another-design","revision":revision}});
        let before = s.doc.clone();
        let reply = s.execute("plugin.run", &args).unwrap();
        assert_eq!(reply["form_validation"]["changed"], true, "a current design token still requires form acknowledgement");
        assert_eq!(s.doc, before);
        assert!(s.undo.is_empty(), "revalidation does not run the entry");

        args["form_guard"]["document_id"] = json!(s.document_id);
        let reply = s.execute("plugin.run", &args).unwrap();
        assert!(reply.get("form_validation").is_none(), "both current guards permit execution");
        assert_eq!(s.undo.len(), 1);
        let actual = s.design_token();
        assert_ne!(expected, actual);
        let after = s.doc.clone();
        let error = s.execute("plugin.run", &args).unwrap_err();
        assert_eq!(
            error.host_error(),
            Some(&HostError::StaleRevision { expected, actual }),
            "a stale explicit design token refuses the call even when its form guard is stale too"
        );
        assert_eq!(s.doc, after);
        assert_eq!(s.undo.len(), 1);
    }

    #[test]
    fn edit_form_apply_replaces_storage_once_and_stale_inputs_are_revalidated() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("sample/plugin.wasm"), storage_component()).unwrap();
        let mut m = manifest();
        m["capabilities"] = json!(["commands@0.1", "storage@0.1"]);
        m["runtime"]["commands"] = json!(["storage.set"]);
        m["runtime"]["data_format"] = json!(1);
        m["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{
            "depth":{"type":"number","minimum":0},
            "edges":{"type":"array","x-solidcraft-selection":{"version":1,"accept":["edge"]}}
        }}}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        s.plugin_state.storage_caller =
            Some(storage::StorageCaller { id: "sample".into(), name: "Sample".into(), version: "0.1.0".into(), data_format: 1 });
        s.execute("storage.set", &json!({"document":{"teeth":12}})).unwrap();
        s.plugin_state.storage_caller = None;
        let before = s.doc.clone();
        let undo = s.undo.len();
        let inspected = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let guard = json!({"document_id":s.document_id,"revision":s.design_revision()});
        let mut args = json!({"path":f.path(),"approval":inspected["approval"],"entry":"make","params":{"depth":5,"edges":[]},"form_guard":guard});
        let prepared = s.scratch().compute_commands(&[("plugin.run".into(), args.clone())], None).unwrap();
        assert_eq!(s.doc, before, "preview/cancel does not commit");
        drop(prepared);
        assert_eq!(s.doc, before, "cancel compares equal");
        let reply = s.execute("plugin.run", &args).unwrap();
        assert!(reply.get("form_validation").is_none(), "{reply}");
        assert_eq!(s.undo.len(), undo + 1);
        assert_eq!(s.execute("storage.get", &json!({"plugin":"sample"})).unwrap()["document"]["teeth"], 24);
        let after = s.doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc, after);
        let history = s.undo.len();
        args["params"]["edges"] = json!([{"type":"edge","body":"deleted","index":0,"point":[0,0,0]}]);
        let stale = s.execute("plugin.run", &args).unwrap();
        assert_eq!(stale["form_validation"]["changed"], true);
        assert_eq!(stale["form_validation"]["errors"]["edges"]["kind"], "missing_pick");
        assert_eq!(s.doc, after);
        assert_eq!(s.undo.len(), history, "revalidation never runs the entry");
        args["params"] = json!({"depth":-1});
        args["form_guard"]["revision"] = json!(s.design_revision());
        let invalid = s.execute("plugin.run", &args).unwrap();
        assert_eq!(invalid["form_validation"]["errors"]["depth"]["kind"], "invalid_value");
        args["params"] = json!({"depth":5});
        let prepared = s.scratch().compute_commands(&[("plugin.run".into(), args)], None).unwrap();
        s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
        assert!(s.accept_commands(prepared).is_err(), "a design change between validation and acceptance rejects the transaction");
    }

    #[test]
    fn a_plugin_keeps_its_data_in_the_design_as_one_undo_step_and_a_newer_formats_data_stays_read_only() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("sample/plugin.wasm"), storage_component()).unwrap();
        let write_manifest = |data_format: u32| {
            let mut m = manifest();
            m["capabilities"] = json!(["commands@0.1", "storage@0.1"]);
            m["runtime"]["commands"] = json!(["storage.set"]);
            m["runtime"]["data_format"] = json!(data_format);
            std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        };
        write_manifest(2);
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(f.0.join("installed"));
        f.run(&mut s).unwrap();
        assert_eq!(s.undo.len(), 1);
        let data = s.execute("storage.get", &json!({"plugin":"sample"})).unwrap();
        assert_eq!((data["document"].clone(), data["format"].clone(), data["name"].clone()), (json!({"teeth":24}), json!(2), json!("Sample")));
        // Only the plugin's own run writes it.
        assert!(s.execute("storage.set", &json!({"document":{}})).unwrap_err().to_string().contains("only a plugin changes its data"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.plugin_data.is_empty(), "one undo takes the data out again");
        s.execute("edit.redo", &json!({})).unwrap();
        let saved = solidcraft_io::write_design_container(&s.doc).unwrap();
        // A plain JSON design can't carry the data: saving one is refused, and nothing is written.
        let json = f.0.join("with-data.json");
        let refused = s.execute("file.save", &json!({"path": json.to_string_lossy(), "format": "json"})).unwrap_err().to_string();
        assert!(refused.contains("plugin files that a .json design can't carry"), "{refused}");
        assert!(!json.exists());

        // Opened where the plugin is not installed, the data is kept and says whose it is.
        let mut elsewhere = Session::new(solidcraft_io::read_design(&saved).unwrap());
        elsewhere.plugin_state.plugins_dir = Some(f.0.join("installed"));
        let listed = elsewhere.execute("storage.list", &json!({})).unwrap();
        assert_eq!(listed["plugins"][0]["status"], "missing");
        assert_eq!((listed["plugins"][0]["name"].clone(), listed["plugins"][0]["read_only"].clone()), (json!("Sample"), json!(true)));
        assert_eq!(elsewhere.doc.plugin_data, s.doc.plugin_data);

        // An older version of the plugin reads the data but never changes it.
        write_manifest(1);
        let before = (s.doc.to_json(), s.doc.plugin_data.clone());
        let refused = f.run(&mut s).unwrap_err().to_string();
        assert!(refused.contains("saved by a newer version of the plugin (data format 2; this version writes 1)"), "{refused}");
        assert_eq!(((s.doc.to_json(), s.doc.plugin_data.clone()), s.undo.len()), (before, 1));
        assert_eq!(s.execute("storage.list", &json!({})).unwrap()["plugins"][0]["status"], "older");
        // A newer one migrates it: the next write saves it in the newer format.
        write_manifest(3);
        assert_eq!(s.execute("storage.list", &json!({})).unwrap()["plugins"][0]["status"], "migrate");
        f.run(&mut s).unwrap();
        assert_eq!(s.execute("storage.get", &json!({"plugin":"sample"})).unwrap()["format"], 3);
        // storage commands need the capability.
        let mut m = manifest();
        m["runtime"]["commands"] = json!(["storage.get"]);
        let refused = parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").unwrap_err().to_string();
        assert!(refused.contains("needs the storage capability"), "{refused}");
    }
    #[test]
    fn named_calls_preserve_input_pass_through() {
        let f = Fixture::new("i32.const 0 i32.const 0 i32.store i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let mut m = manifest();
        m["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"size":{"type":"number","minimum":1}}}}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let approval = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"].clone();
        for inputs in [json!({"colour":"red"}), json!({"size":0}), json!({"size":"large"})] {
            let result = s.execute("sample.make", &json!({"approval":approval,"params":inputs})).unwrap();
            assert_eq!(result["result"], inputs, "direct calls pass inputs to the plugin unchanged");
        }
    }

    #[test]
    fn a_scoped_fresh_session_lists_installed_commands_before_tools_are_requested() {
        let f = Fixture::new("call $box");
        let mut installed = Session::default();
        f.install(&mut installed, json!({}));
        let mut s = Session::default();
        s.plugin_state.plugins_dir = installed.plugin_state.plugins_dir.clone();
        assert!(s.plugin_state.packages.is_empty());
        solidcraft_io::vfs::scoped(Some(Default::default()), || {
            let listed = s.execute("plugin.list", &json!({})).unwrap();
            assert_eq!(listed["plugins"].as_array().unwrap().len(), 1, "{listed}");
            assert_eq!(listed["plugins"][0]["commands"], json!(["sample.make"]));
        });
    }

    /// An installed plugin's entry is a tool: listed with its plugin's text and strict inputs,
    /// called by name as one undo step, gone from the listing once turned off or removed.
    #[test]
    fn installed_entries_are_tools_that_run_as_one_undo_step_until_removed() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        m["description"] = json!("Makes a box");
        m["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"size":{"type":"number"}}}}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        f.install(&mut s, json!({}));
        let listed = s.execute("plugin.tools", &json!({})).unwrap();
        let tools = listed["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "{listed}");
        let tool = &tools[0];
        assert_eq!(
            (&tool["name"], &tool["kind"], &tool["plugin"], &tool["effect"]),
            (&json!("sample.make"), &json!("command"), &json!("sample"), &json!("edit"))
        );
        assert_eq!((&tool["plugin_name"], &tool["description"]), (&json!("Sample"), &json!("Makes a box")));
        assert_eq!(tool["input_schema"]["additionalProperties"], false, "the listed inputs are as strict as the check");
        assert!(tool.get("approval").is_none(), "a listing reads no component");
        let approval = s.execute("plugin.tools", &json!({"name":"sample.make"})).unwrap()["tools"][0]["approval"].clone();
        assert!(approval.is_string());
        let undo = s.undo.len();
        s.execute("sample.make", &json!({"approval": approval, "params": {"size": 2}})).unwrap();
        assert_eq!((s.doc.features.len(), s.undo.len()), (1, undo + 1));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.features.is_empty(), "one undo takes back the whole call");
        s.disabled_plugins.insert("sample".into());
        assert_eq!(s.execute("plugin.tools", &json!({})).unwrap()["tools"], json!([]));
        s.disabled_plugins.clear();
        assert_eq!(s.execute("plugin.tools", &json!({})).unwrap()["tools"].as_array().unwrap().len(), 1);
        s.execute("plugin.remove", &json!({"id":"sample"})).unwrap();
        assert_eq!(s.execute("plugin.tools", &json!({})).unwrap()["tools"], json!([]));
    }

    /// Called by name, a refusal says which grant the plugin lacks and how to approve it, or
    /// which host import a read entry was refused; nothing waits for a dialog.
    #[test]
    fn a_named_call_names_the_missing_grant_and_the_refused_import() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("sample/plugin.wasm"), file_component(true)).unwrap();
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":true}]});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let secret = f.0.join("secret.txt");
        std::fs::write(&secret, b"original").unwrap();
        let mut s = Session::default();
        f.install(&mut s, json!({}));
        let call = |s: &mut Session| {
            let approval = s.execute("plugin.tools", &json!({"name":"sample.make"})).unwrap()["tools"][0]["approval"].clone();
            s.execute("sample.make", &json!({"approval": approval})).unwrap_err().to_string()
        };
        let missing = call(&mut s);
        assert_eq!(
            structured_tool_error(&missing),
            Some(json!({"plugin":"sample","grant":"files.write:project","approve":tools::APPROVE})),
            "{missing}"
        );
        // A read entry with the folder granted for writing is still refused the write.
        m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        f.install(&mut s, json!({"folders":[{"name":"project","path":f.0,"read":true,"write":true}]}));
        let refused = call(&mut s);
        assert_eq!(structured_tool_error(&refused), Some(json!({"plugin":"sample","entry":"make","refused_import":"write-file"})), "{refused}");
        assert_eq!(std::fs::read(&secret).unwrap(), b"original");
        assert!(s.doc.features.is_empty());
        // The same entry run by a person (plugin.run, not by name) is refused the same write.
        let installed = s.plugin_state.packages["sample"].clone();
        let approval = read_package(&installed).unwrap().approval;
        let direct = s.execute("plugin.run", &json!({"path":installed,"approval":approval,"entry":"make"})).unwrap_err();
        assert!(direct.to_string().contains("write-file"), "{direct}");
        assert_eq!(std::fs::read(&secret).unwrap(), b"original");
    }

    #[test]
    fn metadata_only_discovery_preserves_the_explicitly_inspected_path() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("sample/plugin.json"), r#"{"id":"sample","name":"Metadata","version":"1"}"#).unwrap();
        std::fs::remove_file(f.0.join("sample/plugin.wasm")).unwrap();
        let mut s = Session::default();
        let inspected = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        assert_eq!(inspected["manifest"]["name"], "Metadata");
        assert!(read_package(&f.path()).unwrap().component.is_empty());
        s.plugin_state.register_discovered("sample".into(), f.0.join("installed/sample").to_string_lossy().into_owned());
        s.plugin_state.register_discovered("other".into(), "installed/other".into());
        assert_eq!(s.plugin_state.packages["sample"], f.path());
        assert_eq!(s.plugin_state.packages["other"], "installed/other");
        let listed = s.execute("plugin.list", &json!({})).unwrap();
        let entry = listed["plugins"].as_array().unwrap().iter().find(|p| p["manifest"]["id"] == "sample").unwrap();
        assert_eq!(entry["manifest"]["name"], "Metadata");
        assert_eq!(entry["commands"], json!([]));
        assert!(s.execute("engine.commands", &json!({})).is_ok());
        let error = s.execute("plugin.run", &json!({"path":f.path(),"approval":inspected["approval"],"entry":"make"})).unwrap_err();
        assert!(error.to_string().contains("no executable runtime"));
    }
}

#[test]
fn evaluated_mesh_transfer_is_bounded_and_does_not_edit_the_design() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":2,"width":3,"height":4,"body_name":"Box"})).unwrap();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let result = s.execute("model.mesh", &json!({"body":"Box"})).unwrap();
    assert_eq!(result["frame"], "component-local-mm");
    let positions: Vec<[f64; 3]> = serde_json::from_value(result["positions"].clone()).unwrap();
    let triangles: Vec<[u32; 3]> = serde_json::from_value(result["triangles"].clone()).unwrap();
    assert!(!positions.is_empty() && positions.len() <= 8192);
    assert_eq!(triangles.len(), 12);
    assert!(triangles.iter().flatten().all(|i| (*i as usize) < positions.len()));
    for (axis, max) in [2., 3., 4.].into_iter().enumerate() {
        assert_eq!(positions.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min), 0.);
        assert_eq!(positions.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max), max);
        assert!(positions.iter().all(|p| p[axis].is_finite() && (0. ..=max).contains(&p[axis])));
    }
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), undo);
    assert!(s.execute("model.mesh", &json!({"body":"Box","tolerance":0})).is_err());
    assert!(s.execute("model.mesh", &json!({"body":"Missing"})).is_err());
}

#[test]
fn evaluated_mesh_transfer_refuses_an_oversized_input_without_truncation() {
    let mut doc = solidcraft_doc::Document::default();
    doc.add_feature(
        solidcraft_doc::FeatureKind::MeshImport {
            file: "budget.stl".into(),
            meshes: vec![solidcraft_doc::MeshData {
                name: "Many".into(),
                positions: (0..8193).flat_map(|i| [i as f64, (i % 2) as f64, 0.]).collect(),
                triangles: (0..2731).flat_map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(),
                color: None,
                corner_normals: Default::default(),
            }],
        },
        Some("Budget mesh"),
    )
    .unwrap();
    doc.features.last_mut().unwrap().body_names = vec!["Many".into()];
    let mut s = Session::new(doc);
    let before = s.doc.to_json();
    assert!(s.execute("model.mesh", &json!({"body":"Many"})).unwrap_err().to_string().contains("transfer budget"));
    assert_eq!(s.doc.to_json(), before);
}

/// A session holding one mesh body, "Strip": a flat strip of `triangles` triangles, triangle `t`
/// over the points (k, 0, 0) and (k, 1, 0) numbered `t % period` to `t % period + 2`, each
/// triangle with three corners of its own, as an STL gives them. Every other triangle writes its
/// zero coordinates as `-0` (the import writes them as 0, so the body holds one sign).
fn strip_of_unshared_triangles(triangles: u32, period: u32) -> Session {
    let point = |k: u32, zero: f64| [f64::from(k / 2), f64::from(k % 2) + zero, zero];
    let positions: Vec<f64> = (0..triangles)
        .flat_map(|t| {
            let (k, zero) = (t % period, if t.is_multiple_of(2) { 0. } else { -0. });
            [point(k, zero), point(k + 1, zero), point(k + 2, zero)]
        })
        .flatten()
        .collect();
    let mut doc = solidcraft_doc::Document::default();
    doc.add_feature(
        solidcraft_doc::FeatureKind::MeshImport {
            file: "strip.stl".into(),
            meshes: vec![solidcraft_doc::MeshData {
                name: "Strip".into(),
                positions: positions.into(),
                triangles: (0..3 * triangles).collect(),
                color: None,
                corner_normals: Default::default(),
            }],
        },
        Some("Strip mesh"),
    )
    .unwrap();
    doc.features.last_mut().unwrap().body_names = vec!["Strip".into()];
    Session::new(doc)
}

#[test]
fn evaluated_mesh_transfer_shares_the_corners_of_an_unshared_mesh_body() {
    // 3000 triangles hand over 9000 corners, past the 8192-vertex budget unshared (#20627); at
    // 3002 distinct points the body fits
    let mut s = strip_of_unshared_triangles(3000, 3000);
    let before = s.doc.to_json();
    let result = s.execute("model.mesh", &json!({"body":"Strip"})).unwrap();
    let positions: Vec<[f64; 3]> = serde_json::from_value(result["positions"].clone()).unwrap();
    let triangles: Vec<[usize; 3]> = serde_json::from_value(result["triangles"].clone()).unwrap();
    assert_eq!(triangles.len(), 3000);
    assert_eq!(positions.len(), 3002, "one vertex per distinct point");
    let mut distinct = positions.iter().map(|p| p.map(|c| (c + 0.).to_bits())).collect::<Vec<_>>();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), positions.len(), "no point is handed over twice");
    // every triangle keeps its corners where they were: three consecutive points of the strip
    // (the body may turn a triangle round, so its corners are compared as a set)
    let mut corners: Vec<[u64; 3]> = triangles
        .iter()
        .map(|tri| {
            let mut ks = tri.map(|i| {
                let [x, y, z] = positions[i];
                assert_eq!(z, 0.);
                (2. * x + y) as u64
            });
            ks.sort_unstable();
            ks
        })
        .collect();
    corners.sort_unstable();
    let expected: Vec<[u64; 3]> = (0..3000).map(|k| [k, k + 1, k + 2]).collect();
    assert_eq!(corners, expected);
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn shared_corners_count_both_zeros_as_one_point_and_never_share_what_is_not_finite() {
    // a mesh body arrives with -0 already written as 0, so -0 is tested on the sharing itself: a
    // tessellation can compute either sign at a point two faces share
    let corners = [[0., 1., 0.], [-0., 1., -0.], [f64::NAN, 0., 0.], [f64::NAN, 0., 0.], [2., 0., 0.]];
    let (positions, triangles) = crate::cmd::plugin_shared_corners(corners.into_iter(), &[[0, 1, 4], [2, 3, 4]]).unwrap();
    assert_eq!(positions.len(), 4, "{positions:?}");
    assert_eq!(triangles, vec![[0, 0, 3], [1, 2, 3]]);
    assert_eq!(positions[0], [0., 1., 0.]);
    assert!(crate::cmd::plugin_shared_corners(corners.into_iter(), &[[0, 1, 5]]).is_err(), "a corner it does not have");
}

#[test]
fn evaluated_mesh_transfer_past_the_triangle_budget_names_its_count() {
    // 16385 triangles over 3002 distinct points: only the triangle limit is passed
    let mut s = strip_of_unshared_triangles(16385, 3000);
    let before = s.doc.to_json();
    let message = s.execute("model.mesh", &json!({"body":"Strip"})).unwrap_err().to_string();
    assert!(message.contains("transfer budget: 16385 triangles, at most 16384"), "{message}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn evaluated_mesh_transfer_past_the_vertex_budget_after_sharing_is_refused() {
    // 8200 triangles over 8202 distinct points: past the vertex limit even shared
    let mut s = strip_of_unshared_triangles(8200, 8200);
    let before = s.doc.to_json();
    let message = s.execute("model.mesh", &json!({"body":"Strip"})).unwrap_err().to_string();
    assert!(message.contains("transfer budget: more than 8192 distinct vertices"), "{message}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn host_resource_policy_is_exact_user_state_and_never_design_data() {
    let defaults = HostLimits::default();
    assert_eq!((defaults.max_fuel, defaults.max_memory_mib), (100_000_000_000, 128));
    let request = Limits::default();
    assert_eq!((request.fuel, request.memory_mib, request.deadline_ms), (10_000_000, 64, 5000));
    assert!(defaults.check(&request).is_ok());
    for limits in
        [HostLimits { max_fuel: 0, ..defaults }, HostLimits { max_memory_mib: 0, ..defaults }, HostLimits { max_memory_mib: 4097, ..defaults }]
    {
        assert!(limits.validate().is_err());
    }
    let mut m = manifest();
    m["runtime"]["limits"] = json!({"fuel":u64::MAX,"memory_mib":4096,"deadline_ms":30000});
    assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_ok());
    let policy = HostLimits { max_fuel: u64::MAX, max_memory_mib: 4096 };
    assert_eq!(serde_json::from_str::<HostLimits>(&serde_json::to_string(&policy).unwrap()).unwrap(), policy);
    let mut s = Session { plugin_limits: policy, ..Default::default() };
    assert_eq!(s.scratch().plugin_limits, policy);
    s.execute("file.new", &json!({})).unwrap();
    assert_eq!(s.plugin_limits, policy);
    assert!(!s.doc.to_json().contains("max_fuel"));
    assert!(command_group("plugin.configure") == Some("plugin") && catalog_command("plugin.configure").is_none());
}

#[test]
fn command_effects_are_explicit_and_independent_of_undo() {
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    };
    let catalog = host.command_catalog();
    for command in ["sketch.edit", "sketch.finish", "sketch.cancel", "solid.box", "sketch.export_dxf"] {
        assert_eq!(catalog.iter().find(|c| c.name == command).unwrap().effect, Effect::Edit, "{command}");
    }
    for command in [
        "model.mesh",
        "model.faces",
        "model.edges",
        "document.inspect",
        "timeline.inspect",
        "inspect.classify_points",
        "inspect.measure",
        "sketch.inspect",
        "sketch.solve",
    ] {
        assert_eq!(catalog.iter().find(|c| c.name == command).unwrap().effect, Effect::Read, "{command}");
    }
    // A read listener has no command-group grants. It can query exact geometry anyway.
    host.session.execute("solid.box", &json!({"length":2, "width":3, "height":4, "body_name":"Block"})).unwrap();
    host.session.plugin_state.listener_design = Some(Some(host.session.design_token()));
    let before = host.session.doc.clone();
    host.begin_transaction().unwrap();
    let queried = host.run_command("inspect.classify_points", &json!({"body":"Block", "points":[[1,1,1], [0,1,1]]})).unwrap();
    assert_eq!(queried["results"][0]["class"], "inside");
    assert_eq!(queried["results"][1]["class"], "on");
    assert_eq!(queried["results"][1]["normal"], json!([-1., 0., 0.]));
    host.commit_transaction().unwrap();
    assert_eq!(host.session.doc, before);
    // A test-only command: its id goes through a variable so the icon audit, which reads command ids
    // registered under crates/engine/src, doesn't take this fixture for a real command without an icon.
    let id = "sample.new";
    let new = crate::cmd::CommandSpec::new(id, "New", |_, _| Ok(json!({}))).noundo();
    assert_eq!(new.effect, Effect::Edit);
}

#[test]
fn granted_roots_allow_hidden_ancestors_but_exclude_hidden_descendants() {
    let root = solidcraft_io::vfs::temp_dir().join(".settings/AppData/granted");
    let mut session = Session::default();
    session.plugin_state.file_grants = vec![
        FileGrant { name: "project".into(), folder: root.clone(), read: true, write: true },
        FileGrant { name: "hidden".into(), folder: root.join(".hidden"), read: true, write: false },
    ];
    let host = SolidCraftHost {
        session,
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
        forms: Vec::new(),
        form_source: None,
    };
    solidcraft_io::vfs::scoped(None, || {
        for write in [false, true] {
            assert!(host.authorize_file_path(&root.join("public.txt"), write).is_ok());
            assert!(host.authorize_file_path(&root.join(".env"), write).is_err());
            assert!(host.authorize_file_path(&root.join(".hidden/.env"), write).is_err());
            assert!(host.authorize_file_path(&root.parent().unwrap().join("sibling/public.txt"), write).is_err());
        }
        assert!(host.authorize_file_path(&root.join(".hidden/public.txt"), false).is_ok());
        assert!(host.authorize_file_path(&root.join(".hidden/public.txt"), true).is_err());
    });
}

/// Importer plugins: an installed importer opens the files it claims as new, unsaved designs.
#[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod importers {
    use super::*;

    fn importer_manifest(id: &str, extensions: &[&str], input_folder: bool) -> Value {
        json!({"id":id,"name":"Sample Importer","version":"1","capabilities":["formats@0.1"],"runtime":{
            "module":"importer.wasm","commands":[],"entries":[{"name":"convert","kind":"importer","extensions":extensions}],
            "limits":{"fuel":10_000_000,"memory_mib":2,"deadline_ms":5000,"stack_kib":16384},"permissions":{"input_folder":input_folder}}})
    }
    /// A session whose plugins folder holds the original importer fixture as `id`, and a folder of
    /// files to open. The fixture copies the start of the file it imports into the design.
    fn installed(name: &str, plugins: &[(&str, &[&str], bool)]) -> (Session, std::path::PathBuf) {
        let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-importer-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("importer_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        for (id, extensions, input_folder) in plugins {
            let dir = root.join("plugins").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plugin.json"), importer_manifest(id, extensions, *input_folder).to_string()).unwrap();
            std::fs::write(dir.join("importer.wasm"), &component).unwrap();
        }
        std::fs::create_dir_all(root.join("files")).unwrap();
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(root.join("plugins"));
        (s, root)
    }
    /// A small design's file bytes: what the fixture hands back as the imported design.
    fn design() -> Vec<u8> {
        let mut made = Session::new(solidcraft_doc::Document::new("Bracket"));
        made.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
        solidcraft_io::write_design(&made.doc).unwrap()
    }
    fn put(root: &Path, name: &str, bytes: &[u8]) -> String {
        let path = root.join("files").join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_claimed_file_opens_as_a_new_unsaved_design_with_its_loss_report() {
        let (mut s, root) = installed("open", &[("smp", &["smp", "stl"], false)]);
        let listed = s.execute("plugin.list", &json!({})).unwrap();
        assert_eq!(listed["importers"][0]["id"], "smp.convert");
        assert_eq!(listed["plugins"][0]["commands"], json!([]), "an importer is not a command");
        assert_eq!(listed["plugins"][0]["manifest"]["runtime"]["entries"][0]["effect"], "open", "it opens a new design, it does not edit");
        let opened = s.execute("doc.open", &json!({"path": put(&root, "bracket.smp", &design())})).unwrap();
        assert_eq!(
            (opened["format"].as_str(), opened["plugin"].as_str(), opened["bodies"].as_u64()),
            (Some("smp.convert"), Some("Sample Importer"), Some(1))
        );
        assert_eq!(opened["loss_report"]["summary"], "Copied the file");
        assert_eq!(opened["loss_report"]["items"][0]["severity"], "dropped");
        assert_eq!(s.doc.name, "Bracket");
        assert!(s.path.is_none() && s.is_dirty() && s.undo.is_empty(), "a new design, not saved anywhere yet");
        // SolidCraft reads .stl itself: doc.open keeps its own reader, file.import uses the plugin.
        let stl = put(&root, "part.stl", &design());
        assert!(s.execute("doc.open", &json!({"path": stl})).map_or(true, |r| r.get("loss_report").is_none()));
        assert_eq!(s.execute("file.import", &json!({"path": stl})).unwrap()["format"], "smp.convert");
        assert_eq!(s.execute("doc.open", &json!({"path": stl, "format": "smp.convert"})).unwrap()["bodies"], 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_documents_do_not_share_the_converter_templates_identity() {
        let (mut s, root) = installed("distinct-identities", &[("smp", &["smp"], false)]);
        let bytes = design();
        let original_identity = s.document_id.clone();
        let first = put(&root, "first.smp", &bytes);
        let second = put(&root, "second.smp", &bytes);
        s.execute("file.import", &json!({"path":first})).unwrap();
        let first = s.design_token();
        s.execute("file.import", &json!({"path":second})).unwrap();
        let second = s.design_token();
        assert_ne!(first.document_id, original_identity);
        assert_ne!(second.document_id, original_identity);
        assert_ne!(first.document_id, second.document_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn conditional_importer(name: &str) -> (Session, std::path::PathBuf) {
        let (s, root) = installed(name, &[("smp", &["smp"], false)]);
        let mut manifest = importer_manifest("smp", &["smp"], false);
        manifest["runtime"]["entries"][0]["input_schema"] = json!({"type":"object","properties":{
            "mode":{"type":"boolean"},
            "count":{"type":"integer","x-solidcraft-visible-when":{"field":"mode","eq":true}},
            "size":{"type":"string","oneOf":[
                {"const":"small","x-solidcraft-when":{"field":"mode","eq":true}},
                {"const":"large","x-solidcraft-when":{"field":"mode","eq":false}}]}
        }});
        std::fs::write(root.join("plugins/smp/plugin.json"), manifest.to_string()).unwrap();
        (s, root)
    }

    #[test]
    fn importer_options_without_conditions_validate_before_entering_wasm() {
        let (mut s, root) = installed("plain-options", &[("smp", &["smp"], false)]);
        let mut manifest = importer_manifest("smp", &["smp"], false);
        manifest["runtime"]["entries"][0]["input_schema"] = json!({"type":"object","required":["width"],"properties":{
            "width":{"type":"number","minimum":1,"maximum":10}
        }});
        std::fs::write(root.join("plugins/smp/plugin.json"), manifest.to_string()).unwrap();
        let file = put(&root, "bracket.smp", &design());
        let mut importer = importer_for(&s, &file, None).unwrap().unwrap();
        let before = s.doc.to_json();
        for options in [json!({"width":0}), json!({}), json!({"width":"2"}), json!({"width":11})] {
            let error = import_file(&mut s, &importer, &file, &options).unwrap_err();
            assert!(matches!(error.host_error(), Some(HostError::InvalidInput { field, .. }) if field == "params.width"), "{error}");
            assert_eq!(s.doc.to_json(), before);
        }
        assert!(import_file(&mut s, &importer, &file, &json!({"width":2})).is_ok());
        // A cached catalog schema must not weaken the schema of the package being executed.
        importer.input_schema = json!({});
        let error = import_file(&mut s, &importer, &file, &json!({"width":0})).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::InvalidInput { field, .. }) if field == "params.width"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn importer_options_omit_hidden_fields_before_entering_wasm() {
        let (mut s, root) = conditional_importer("conditional-hidden");
        // The guest traps unless the serialized options contain only {"mode":false}.
        let (package, version) = API.split_once('@').unwrap();
        let fixture = include_str!("importer_fixture.wat").replace("\r\n", "\n").replace("__PACKAGE__", package).replace("__VERSION__", version);
        let signature = "(func $import (;3;) (type 3) (param i32 i32 i32 i32 i64 i32 i32) (result i32)\n";
        assert!(fixture.contains(signature));
        let fixture = fixture.replacen(
            signature,
            &format!("{signature}      local.get 6\n      i32.const 14\n      i32.ne\n      if\n        unreachable\n      end\n"),
            1,
        );
        std::fs::write(root.join("plugins/smp/importer.wasm"), wat::parse_str(fixture).unwrap()).unwrap();
        let file = put(&root, "bracket.smp", &design());
        let importer = importer_for(&s, &file, None).unwrap().unwrap();
        let imported = import_file(&mut s, &importer, &file, &json!({"mode":false,"count":"hidden"}));
        assert!(imported.is_ok(), "{imported:?}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn importer_options_reject_unavailable_choices_before_entering_wasm() {
        let (mut s, root) = conditional_importer("conditional-choice");
        let file = put(&root, "bracket.smp", &design());
        let importer = importer_for(&s, &file, None).unwrap().unwrap();
        let error = import_file(&mut s, &importer, &file, &json!({"mode":false,"size":"small"})).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::InvalidInput { field, .. }) if field == "params.size"), "{error}");
        assert!(import_file(&mut s, &importer, &file, &json!({"mode":false,"size":"large"})).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// An agent that may use only the folder of its input file still sees and uses the installed
    /// importer: the registry is the app's, not the agent's (finding 2 of a new-user walk). The
    /// input file itself is still checked, and refused with the path named.
    #[test]
    fn an_agent_with_only_its_input_folder_lists_describes_and_uses_an_installed_importer() {
        let (mut s, root) = installed("scoped", &[("smp", &["smp"], false)]);
        let file = put(&root, "bracket.smp", &design());
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let outside = elsewhere.join("other.smp");
        std::fs::write(&outside, design()).unwrap();
        let scope = solidcraft_io::vfs::PathScope::new([root.join("files")]);
        solidcraft_io::vfs::scoped(Some(scope), || {
            let listed = s.execute("plugin.list", &json!({})).unwrap();
            assert_eq!(listed["importers"][0]["id"], "smp.convert", "{listed}");
            assert_eq!(listed["plugins"].as_array().unwrap().len(), 1, "{listed}");
            let tools = s.execute("plugin.tools", &json!({})).unwrap();
            assert_eq!(tools["tools"][0]["name"], "smp.convert", "{tools}");
            // Described as the tool tools/list gives (finding 9): its schema, not "unknown command".
            let described = s.execute("engine.describe", &json!({"command":"smp.convert"})).unwrap();
            assert_eq!(described["kind"], "importer");
            assert_eq!(described["tool"]["input"], tools["tools"][0]["input_schema"]);
            assert_eq!(described["input"]["required"], json!(["path"]));
            assert_eq!(s.execute("doc.open", &json!({"path": file})).unwrap()["format"], "smp.convert");
            assert_eq!(s.execute("file.import", &json!({"path": file, "format": "smp.convert"})).unwrap()["bodies"], 1);
            let refused = s.execute("file.import", &json!({"path": outside.to_string_lossy(), "format": "smp.convert"})).unwrap_err().to_string();
            assert!(refused.contains("other.smp") || refused.contains("elsewhere"), "the input file's own refusal names it: {refused}");
            assert!(!refused.contains("no installed"), "{refused}");
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_trap_fails_that_one_import_and_a_disabled_or_ambiguous_importer_opens_nothing() {
        let (mut s, root) = installed("trap", &[("smp", &["smp"], false)]);
        s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
        let before = s.doc.to_json();
        let failure = s.execute("doc.open", &json!({"path": put(&root, "broken.smp", b"T")})).unwrap_err().to_string();
        assert!(failure.starts_with("Sample Importer could not open broken.smp: it stopped on an error inside the plugin ("), "{failure}");
        assert!(failure.contains("UnreachableCodeReached") && failure.ends_with("Report this to its author."), "{failure}");
        assert_eq!(s.doc.to_json(), before, "the open design is untouched");
        assert_eq!(s.execute("doc.open", &json!({"path": put(&root, "good.smp", &design())})).unwrap()["bodies"], 1);
        s.disabled_plugins.insert("smp".into());
        assert!(s.execute("file.import", &json!({"path": put(&root, "off.smp", &design())})).is_err());
        let (mut two, root2) = installed("two", &[("smp", &["smp"], false), ("other", &["smp"], false)]);
        let ambiguous = two.execute("doc.open", &json!({"path": put(&root2, "x.smp", &design())})).unwrap_err().to_string();
        assert!(ambiguous.contains("other.convert, smp.convert"), "{ambiguous}");
        assert!(two.execute("doc.open", &json!({"path": put(&root2, "x.smp", &design()), "format": "other.convert"})).is_ok());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(root2).unwrap();
    }

    #[test]
    fn an_import_failure_names_the_plugin_and_the_file_and_says_not_supported_plainly() {
        let (mut s, root) = installed("errors", &[("smp", &["smp"], false)]);
        // A valid file the importer does not convert: "does not support", not "invalid".
        let error = s.execute("doc.open", &json!({"path": put(&root, "sketch.smp", b"U")})).unwrap_err();
        assert_eq!(error.to_string(), "Sample Importer does not support sketch.smp: the model has no solids");
        assert_eq!(error.host_error(), Some(&HostError::Unsupported { reason: "the model has no solids".into() }));
        // An importer built before 0.2 could only say invalid-input: still named, without the jargon.
        let error = s.execute("doc.open", &json!({"path": put(&root, "broken.smp", b"I")})).unwrap_err();
        assert_eq!(error.to_string(), "Sample Importer could not open broken.smp: the model has no solids");
        // More memory than plugins may use: both numbers, and the two ways to allow it.
        let path = root.join("plugins/smp/plugin.json");
        let mut m = importer_manifest("smp", &["smp"], false);
        m["runtime"]["limits"]["memory_mib"] = json!(2048);
        std::fs::write(&path, m.to_string()).unwrap();
        let error = s.execute("doc.open", &json!({"path": put(&root, "big.smp", &design())})).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Sample Importer could not open big.smp: it asks for 2048 MiB of memory, and plugins may use 128 MiB. Install the plugin again and allow its memory request, or raise Max plugin memory (MiB) in Preferences › Plugins (in solidcraft-cli, max_memory_mib in the --plugin-limits file)."
        );
        // The time limit, in seconds.
        m["runtime"]["limits"] = json!({"fuel":100_000_000_000_u64,"memory_mib":2,"deadline_ms":50,"stack_kib":16384});
        std::fs::write(&path, m.to_string()).unwrap();
        let error = s.execute("doc.open", &json!({"path": put(&root, "slow.smp", b"L")})).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Sample Importer could not open slow.smp: it ran longer than its time limit of 0.05 s and was stopped. Ask its author to raise its time limit."
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_assembly_reads_the_files_beside_it_only_when_asked_and_granted_at_install() {
        for (asked, granted) in [(false, false), (true, false), (true, true)] {
            let (mut s, root) = installed(&format!("folder-{asked}-{granted}"), &[("smp", &["smp"], asked)]);
            let package = read_package(&root.join("plugins/smp").to_string_lossy()).unwrap();
            if granted {
                s.plugin_state
                    .grants
                    .insert("smp".into(), Grants { package_hash: Some(package.approval.clone()), input_folder: true, ..Default::default() });
            }
            put(&root, "part.bin", &design());
            let reply = s.execute("doc.open", &json!({"path": put(&root, "assembly.smp", b"S")}));
            match granted {
                false => assert!(reply.unwrap_err().to_string().contains("input_folder"), "asked {asked}"),
                true => assert_eq!(reply.unwrap()["bodies"], 1),
            }
            // Approving the folder for an importer that doesn't ask for it is refused.
            let approval = Grants { input_folder: true, ..Default::default() };
            assert_eq!(validate_grants(&package.manifest, &approval).is_ok(), asked);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    /// A session that keeps its grants in the store of `plugins`, as the command line's do.
    fn stored_session(plugins: &Path) -> Session {
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(plugins.to_path_buf());
        s.plugin_state.stored = Some(Default::default());
        s
    }

    #[test]
    fn an_import_uses_approvals_from_an_earlier_session_until_the_component_changes_or_is_revoked() {
        let (_, root) = installed("stored", &[]);
        let plugins = root.join("plugins");
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("importer_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        let manifest = importer_manifest("smp", &["smp"], true).to_string();
        let zip = |component: &[u8]| solidcraft_io::safe_zip::write(&[("plugin.json", manifest.as_bytes()), ("importer.wasm", component)]).unwrap();
        let original = put(&root, "smp.zip", &zip(&component));
        put(&root, "part.bin", &design());
        let assembly = put(&root, "assembly.smp", b"S");
        let open = |s: &mut Session| s.execute("doc.open", &json!({"path": assembly}));

        // Installed and approved in one session, used by the next.
        stored_session(&plugins).execute("plugin.install", &json!({"path": original, "approve": {"input_folder": true}})).unwrap();
        assert!(plugins.join(".grants/smp.json").is_file());
        let mut later = stored_session(&plugins);
        assert_eq!(open(&mut later).unwrap()["bodies"], 1);
        let listed = later.execute("plugin.grants", &json!({})).unwrap();
        assert_eq!((listed["grants"][0]["id"].as_str(), listed["grants"][0]["status"].as_str()), (Some("smp"), Some("current")));
        assert_eq!(listed["grants"][0]["input_folder"], true);
        // A session that does not keep grants between runs gets nothing from the store.
        let mut session_only = Session::default();
        session_only.plugin_state.plugins_dir = Some(plugins.clone());
        assert!(open(&mut session_only).unwrap_err().to_string().contains("input_folder"));

        // The installed component changes: refused until it is installed and approved again.
        let mut changed = component.clone();
        changed.extend([0, 4, 3, b'n', b'e', b'w']);
        std::fs::write(plugins.join("smp/importer.wasm"), &changed).unwrap();
        let mut after_change = stored_session(&plugins);
        assert!(open(&mut after_change).unwrap_err().to_string().contains("re-approval"));
        assert_eq!(after_change.execute("plugin.grants", &json!({})).unwrap()["grants"][0]["status"], "changed");
        let reviewed = put(&root, "changed.zip", &zip(&changed));
        after_change.execute("plugin.install", &json!({"path": reviewed, "replace": true, "approve": {"input_folder": true}})).unwrap();
        assert_eq!(open(&mut stored_session(&plugins)).unwrap()["bodies"], 1);

        // Revoked in one session: the next runs it with nothing granted.
        let revoked = stored_session(&plugins).execute("plugin.revoke", &json!({"id": "smp"})).unwrap();
        assert_eq!(revoked["revoked"], true);
        assert!(!plugins.join(".grants/smp.json").exists());
        let mut after_revoke = stored_session(&plugins);
        assert_eq!(after_revoke.execute("plugin.grants", &json!({})).unwrap()["grants"], json!([]));
        assert!(open(&mut after_revoke).unwrap_err().to_string().contains("input_folder"));
        assert_eq!(after_revoke.execute("doc.open", &json!({"path": put(&root, "one.smp", &design())})).unwrap()["bodies"], 1);
        assert_eq!(after_revoke.execute("plugin.revoke", &json!({"id": "smp"})).unwrap()["revoked"], false);
        assert!(after_revoke.execute("plugin.revoke", &json!({"id": "../smp"})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_removal_whose_approvals_cannot_be_taken_back_keeps_the_package_installed() {
        let (_, root) = installed("stored-remove", &[]);
        let plugins = root.join("plugins");
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("importer_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        let manifest = importer_manifest("smp", &["smp"], true).to_string();
        let zip = solidcraft_io::safe_zip::write(&[("plugin.json", manifest.as_bytes()), ("importer.wasm", &component)]).unwrap();
        let original = put(&root, "smp.zip", &zip);
        stored_session(&plugins).execute("plugin.install", &json!({"path": original, "approve": {"input_folder": true}})).unwrap();
        let record = plugins.join(".grants/smp.json");
        // A file scope that reaches the package but not the grant store: the record cannot be
        // deleted, so nothing is removed and the error says why.
        let mut s = stored_session(&plugins);
        let scope = solidcraft_io::vfs::PathScope::new([plugins.join("smp")]);
        let refused = solidcraft_io::vfs::scoped(Some(scope), || s.execute("plugin.remove", &json!({"id": "smp"})));
        assert!(refused.unwrap_err().to_string().contains("approvals"));
        assert!(record.is_file() && plugins.join("smp/plugin.json").is_file());
        s.execute("plugin.remove", &json!({"id": "smp"})).unwrap();
        assert!(!record.exists() && !plugins.join("smp").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Exporter plugins: an export slot writes the open design as a file of an extension it claims.
#[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod exporters {
    use super::*;

    fn exporter_manifest(commands: Value) -> Value {
        let slot = |name: &str, extension: &str| json!({"name":name,"kind":"exporter","extensions":[extension]});
        json!({"id":"smp","name":"Sample Exporter","version":"1","capabilities":["formats@0.1"],"runtime":{
            "module":"exporter.wasm","commands":commands,
            "entries":[slot("copy","smp"),slot("error","bad"),slot("trap","bad"),slot("loop","bad"),slot("write","bad")],
            "limits":{"fuel":10_000_000,"memory_mib":2,"deadline_ms":5000}}})
    }
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(name: &str) -> Self {
            let root = Self(solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-exporter-{}-{name}", std::process::id())));
            let _ = std::fs::remove_dir_all(&root.0);
            root
        }
    }
    impl std::ops::Deref for Fixture {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A session with a box whose plugins folder holds the original exporter fixture, and a folder
    /// to export into. The fixture's `copy` slot writes the reply of `document.inspect`.
    fn installed(name: &str) -> (Session, Fixture) {
        let root = Fixture::new(name);
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("exporter_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        let dir = root.join("plugins/smp");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.json"), exporter_manifest(json!(["document.inspect"])).to_string()).unwrap();
        std::fs::write(dir.join("exporter.wasm"), &component).unwrap();
        std::fs::create_dir_all(root.join("files")).unwrap();
        let mut s = Session::new(solidcraft_doc::Document::new("Bracket"));
        s.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
        s.plugin_state.plugins_dir = Some(root.join("plugins"));
        (s, root)
    }
    fn at(root: &Path, name: &str) -> String {
        root.join("files").join(name).to_string_lossy().into_owned()
    }
    fn files(root: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(root.join("files")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn the_exporter_fixture_cleans_up_on_return_and_unwind() {
        for unwind in [false, true] {
            let fixture = Fixture::new(if unwind { "cleanup-unwind" } else { "cleanup-return" });
            let path = fixture.0.clone();
            let result = std::panic::catch_unwind(move || {
                let root = fixture;
                std::fs::create_dir_all(root.join("plugins")).unwrap();
                std::fs::write(root.join("partial.wasm"), b"partial setup").unwrap();
                if unwind {
                    panic!("fixture setup interrupted");
                }
            });
            assert_eq!(result.is_err(), unwind);
            assert!(!path.exists(), "fixture files remain at {}", path.display());
        }
    }

    #[test]
    fn an_installed_exporter_id_takes_precedence_over_a_native_file_name() {
        let (mut s, root) = installed("native-name");
        let mut manifest = exporter_manifest(json!(["document.inspect"]));
        manifest["runtime"]["entries"][0]["name"] = json!("step");
        std::fs::write(root.join("plugins/smp/plugin.json"), manifest.to_string()).unwrap();
        let reply = s.execute("file.export", &json!({"path": at(&root, "bracket.step"), "format": "smp.step"})).unwrap();
        assert_eq!(reply["format"], "smp.step");
        let written: Value = serde_json::from_slice(&std::fs::read(at(&root, "bracket.step")).unwrap()).unwrap();
        let mut inspected = s.execute("document.inspect", &json!({})).unwrap();
        inspected["undo"] = json!(0);
        assert_eq!(written, inspected);
    }

    #[test]
    fn an_export_slot_writes_what_it_read_of_the_design_and_changes_nothing() {
        let (mut s, root) = installed("round-trip");
        let listed = s.execute("plugin.list", &json!({})).unwrap();
        assert_eq!(listed["exporters"][0]["id"], "smp.copy");
        assert_eq!(listed["plugins"][0]["commands"], json!([]), "an exporter is not a command");
        // An agent's output folder grants no access to the installed package itself.
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.join("files")])), || {
            let tools = s.execute("plugin.tools", &json!({"name":"smp.copy"})).unwrap();
            assert_eq!(tools["tools"][0]["kind"], "exporter");
            assert!(tools["tools"][0].get("approval").is_none(), "format tools use the package's stored grants");
            let described = s.execute("engine.describe", &json!({"command":"smp.copy"})).unwrap();
            assert_eq!(described["kind"], "exporter");
            assert_eq!(described["tool"]["input"], tools["tools"][0]["input_schema"]);
        });
        let (before, undo) = (s.doc.to_json(), s.undo.len());
        let mut inspected = s.execute("document.inspect", &json!({})).unwrap();
        // The plugin reads a copy of the session that carries no undo history, so it counts none.
        assert_eq!(inspected["undo"], 1);
        inspected["undo"] = json!(0);
        // The extension picks the slot; the file holds exactly what the plugin read.
        let reply = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.join("files")])), || {
            s.execute("file.export", &json!({"path": at(&root, "bracket.smp")}))
        })
        .unwrap();
        assert_eq!((reply["format"].as_str(), reply["plugin"].as_str()), (Some("smp.copy"), Some("Sample Exporter")));
        let written: Value = serde_json::from_slice(&std::fs::read(at(&root, "bracket.smp")).unwrap()).unwrap();
        assert_eq!(written, inspected);
        assert_eq!(reply["bytes"].as_u64(), Some(std::fs::metadata(at(&root, "bracket.smp")).unwrap().len()));
        assert_eq!((s.doc.to_json(), s.undo.len()), (before, undo), "an export leaves the design and its history alone");
        // `format` names a slot even for an extension SolidCraft writes; without it, SolidCraft does.
        assert_eq!(s.execute("file.export", &json!({"path": at(&root, "copy.json"), "format": "smp.copy"})).unwrap()["format"], "smp.copy");
        assert_eq!(s.execute("file.export", &json!({"path": at(&root, "own.json")})).unwrap()["format"], "Design");
        assert!(s.execute("file.export", &json!({"path": at(&root, "own.stl"), "options": {}})).is_err(), "options are a plugin's");
        assert!(s.execute("file.export", &json!({"path": at(&root, "b.smp"), "bodies": ["Box"]})).is_err());
        assert_eq!(files(&root), ["bracket.smp", "copy.json", "own.json"]);
        // A disabled plugin writes nothing.
        s.disabled_plugins.insert("smp".into());
        assert!(s.execute("file.export", &json!({"path": at(&root, "off.smp")})).is_err());
    }

    #[test]
    fn exporter_hooks_fill_omitted_options_preserve_callers_and_fail_before_writing() {
        let (mut s, root) = installed("dialog-hook");
        let mut manifest = exporter_manifest(json!([]));
        manifest["capabilities"] = json!(["formats@0.2", "dialog@0.1"]);
        manifest["runtime"]["entries"] = json!([{"name":"copy","kind":"exporter","extensions":["smp"],"dialog_hook":true,
        "input_schema":{"type":"object","required":["width"],"properties":{
            "width":{"type":"number","minimum":1},
            "size":{"type":"string","oneOf":[{"const":"small"},{"const":"large"}]}
        }}}]);
        std::fs::write(root.join("plugins/smp/plugin.json"), manifest.to_string()).unwrap();
        let reply = r#"{"width":{"default":24},"size":{"choices":["large"]}}"#;
        // The command fixture echoes its inputs. Give that echo the export-file signature
        // (string and list<u8> share the canonical layout), retaining its update-fields export.
        let source = include_str!("../../../craft-plugin-wasm/src/dialog_fixture.wat")
            .replace(
                r##"(type (;7;) (func (param "entry" string) (param "params" $"#type4 json") (result 6)))"##,
                r##"(type $export-bytes (list u8))
                (type $export-result (result $export-bytes (error $"#type5 plugin-error")))
                (type $export-type (func (param "slot" string) (param "options" $"#type4 json") (result $export-result)))"##,
            )
            .replace("(func $execute (;1;) (type 7)", "(func $execute (;1;) (type $export-type)")
            .replace(r#"(export "execute" (func $execute))"#, r#"(export "export-file" (func $execute))"#)
            .replace(
                r#"(export "__PACKAGE__/entry@__VERSION__" (instance $command))"#,
                r#"(export "__PACKAGE__/formats-provider@__VERSION__" (instance $command))"#,
            )
            .replace("__PACKAGE__", "craft:plugin")
            .replace("__VERSION__", "0.2.0")
            .replace("__REPLY_LEN__", &reply.len().to_string())
            .replace("__REPLY__", &reply.replace('"', "\\22"));
        let component_path = root.join("plugins/smp/exporter.wasm");
        std::fs::write(&component_path, wat::parse_str(source.replace("__HOOK_CODE__", "")).unwrap()).unwrap();
        let tools = s.execute("plugin.tools", &json!({"name":"smp.copy"})).unwrap();
        let schema = &tools["tools"][0]["input_schema"];
        check_input(schema, &json!({"path":at(&root, "defaults.smp"),"options":{}})).unwrap();
        assert!(schema["properties"]["options"].get("required").is_none());
        for (name, options, width) in [("defaults.smp", json!({}), 24), ("explicit.smp", json!({"width":12}), 12)] {
            s.execute("file.export", &json!({"path":at(&root, name),"options":options})).unwrap();
            let written: Value = serde_json::from_slice(&std::fs::read(at(&root, name)).unwrap()).unwrap();
            assert_eq!(written["width"], width);
        }
        let path = at(&root, "refused.smp");
        std::fs::write(&path, b"original").unwrap();
        assert!(s.execute("file.export", &json!({"path":path,"options":{"size":"small"}})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        for code in ["unreachable", "(loop $forever br $forever)", "call $box"] {
            std::fs::write(&component_path, wat::parse_str(source.replace("__HOOK_CODE__", code)).unwrap()).unwrap();
            assert!(s.execute("file.export", &json!({"path":path})).is_err(), "{code}");
            assert_eq!(std::fs::read(&path).unwrap(), b"original");
        }
    }

    #[test]
    fn an_export_that_errors_traps_or_runs_away_leaves_the_folder_as_it_was() {
        let (mut s, root) = installed("failures");
        std::fs::write(at(&root, "keep.bad"), b"original").unwrap();
        for (slot, says) in [("error", "refused this design"), ("trap", "UnreachableCodeReached"), ("loop", "fuel")] {
            for name in ["keep.bad", "new.bad"] {
                let failure = s.execute("file.export", &json!({"path": at(&root, name), "format": format!("smp.{slot}")})).unwrap_err().to_string();
                assert!(failure.contains(says), "{slot}: {failure}");
                assert!(failure.starts_with(&format!("Sample Exporter could not export {name}:")), "{failure}");
                if slot == "loop" {
                    assert!(failure.contains("Ask its author to raise its fuel limit."), "{failure}");
                }
            }
        }
        assert_eq!(std::fs::read(at(&root, "keep.bad")).unwrap(), b"original");
        assert_eq!(files(&root), ["keep.bad"], "no new file and no partial or temporary file");
        // The next export gets a fresh instance.
        assert!(s.execute("file.export", &json!({"path": at(&root, "after.smp")})).is_ok());
    }

    #[test]
    fn an_exporter_reaches_no_other_path_and_may_only_read() {
        let (mut s, root) = installed("no-files");
        let failure = s.execute("file.export", &json!({"path": at(&root, "x.bad"), "format": "smp.write"})).unwrap_err().to_string();
        assert!(failure.contains("file.export"), "{failure}");
        assert_eq!(files(&root), Vec::<String>::new());
        assert!(!Path::new("stolen.txt").exists());
        // A manifest asking for a command that writes, edits or reaches a file is refused at once.
        for commands in [json!(["file.export"]), json!(["solid.box"])] {
            let refused = parse_manifest(exporter_manifest(commands.clone()).to_string().as_bytes(), "smp");
            assert!(refused.is_err(), "{commands}");
        }
        let mut grants = exporter_manifest(json!([]));
        grants["runtime"]["permissions"] = json!({"files":[{"name":"out","write":true}]});
        assert!(parse_manifest(grants.to_string().as_bytes(), "smp").is_err());
        // The caller's file scope decides where an export may go, before any plugin code runs.
        let outside = solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.join("plugins")])), || {
            s.execute("file.export", &json!({"path": at(&root, "y.smp")}))
        });
        assert!(outside.is_err());
        assert_eq!(files(&root), Vec::<String>::new());
    }
}

mod install_zip {
    use super::*;
    use crate::plugin::install::{InstallError, unpack};
    use solidcraft_io::safe_zip;

    fn manifest_bytes(id: &str, capabilities: Value) -> Vec<u8> {
        let mut m = manifest();
        m["id"] = json!(id);
        m["name"] = json!("Gears");
        m["capabilities"] = capabilities;
        serde_json::to_vec(&m).unwrap()
    }
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        safe_zip::write(files).unwrap()
    }
    fn gears(version: &str) -> Vec<u8> {
        let mut m: Value = serde_json::from_slice(&manifest_bytes("gears", json!(["commands@0.1"]))).unwrap();
        m["version"] = json!(version);
        zip(&[("gears-0.1.0/plugin.json", &serde_json::to_vec(&m).unwrap()[..]), ("gears-0.1.0/plugin.wasm", b"\0asm"), ("__MACOSX/._x", b"")])
    }
    /// A session whose plugins folder is a fresh temporary one, and the folder of the zip files.
    fn session(name: &str) -> (Session, std::path::PathBuf) {
        let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-install-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("zips")).unwrap();
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(root.join("plugins"));
        (s, root)
    }
    fn put(root: &Path, name: &str, bytes: &[u8]) -> String {
        let path = root.join("zips").join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn invalid_swatches_warn_in_check_and_install_review_but_install_succeeds() {
        let (mut s, root) = session("swatches");
        let mut m: Value = serde_json::from_slice(&manifest_bytes("gears", json!(["commands@0.1"]))).unwrap();
        m["runtime"]["entries"] = json!([{"name":"make","input_schema":{"type":"object","properties":{"finish":{"type":"string","oneOf":[
            {"const":"oak","x-solidcraft-swatch":"bad.png"}, {"const":"plain","x-solidcraft-swatch":"../outside.png"}
        ]}}}}]);
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        let zip = zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", &component), ("bad.png", b"not an image")]);
        let path = put(&root, "swatches.zip", &zip);
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true})).unwrap();
        assert_eq!(dry["param_warnings"].as_array().unwrap().len(), 2);
        let checked = s.execute("plugin.check", &json!({"path":path})).unwrap();
        assert_eq!(checked["ok"], true, "{checked}");
        let warnings: Vec<_> = checked["problems"].as_array().unwrap().iter().filter(|p| p["field"] == "x-solidcraft-swatch").collect();
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().all(|p| p["severity"] == "warning"));
        let installed = s.execute("plugin.install", &json!({"path":path})).unwrap();
        assert_eq!(installed["installed"], true);
        let folder = root.join("plugins/gears");
        let checked_folder = s.execute("plugin.check", &json!({"path":folder})).unwrap();
        assert_eq!(checked_folder["ok"], true, "{checked_folder}");
        // Builds without plugin execution also report an unrelated component warning.
        let folder_warnings: Vec<_> = checked_folder["problems"].as_array().unwrap().iter().filter(|p| p["field"] == "x-solidcraft-swatch").collect();
        assert_eq!(folder_warnings.len(), 2);
        assert!(folder_warnings.iter().all(|p| p["severity"] == "warning"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_valid_zip_installs_and_its_entries_resolve() {
        let (mut s, root) = session("valid");
        let path = put(&root, "gears.zip", &gears("0.1.0"));
        let dry = s.execute("plugin.install", &json!({"path": path, "dry_run": true})).unwrap();
        assert_eq!((dry["installed"].clone(), root.join("plugins/gears").exists()), (json!(false), false));
        // A changed auxiliary file keeps the manifest-and-component hash but not the archive's:
        // installing with the reviewed archive hash refuses it and publishes nothing.
        let (m, w) = (
            serde_json::to_vec(&{
                let mut m: Value = serde_json::from_slice(&manifest_bytes("gears", json!(["commands@0.1"]))).unwrap();
                m["version"] = json!("0.1.0");
                m
            })
            .unwrap(),
            b"\0asm",
        );
        let reviewed = zip(&[("gears-0.1.0/plugin.json", &m[..]), ("gears-0.1.0/plugin.wasm", w), ("gears-0.1.0/data.txt", b"reviewed")]);
        let changed = zip(&[("gears-0.1.0/plugin.json", &m[..]), ("gears-0.1.0/plugin.wasm", w), ("gears-0.1.0/data.txt", b"changed")]);
        // Not aux.zip. Windows 10 keeps AUX for a device, whatever the extension.
        let aux = put(&root, "auxiliary.zip", &reviewed);
        let aux_dry = s.execute("plugin.install", &json!({"path": aux, "dry_run": true})).unwrap();
        std::fs::write(&aux, &changed).unwrap();
        let error = s.execute("plugin.install", &json!({"path": aux, "approval": aux_dry["approval"], "archive": aux_dry["archive"]})).unwrap_err();
        assert!(error.to_string().contains("archive"), "{error}");
        assert!(!root.join("plugins/gears").exists() && s.plugin_state.grants.is_empty());
        let reply = s.execute("plugin.install", &json!({"path": path, "approval": dry["approval"], "archive": dry["archive"]})).unwrap();
        assert_eq!(reply["approval"], dry["approval"]);
        assert_eq!(reply["archive"], dry["archive"]);
        assert_eq!(reply["id"], "gears");
        assert_eq!(reply["name"], "Gears");
        assert_eq!(reply["version"], "0.1.0");
        assert_eq!(reply["capabilities"], json!(["commands@0.1"]));
        assert_eq!(reply["entries"], json!(["gears.make"]));
        assert_eq!(reply["commands"], json!(["solid.box"]));
        assert!(reply["permissions"].is_object() && reply.get("replaces").is_none());
        assert_eq!(std::fs::read(root.join("plugins/gears/plugin.wasm")).unwrap(), b"\0asm");
        assert!(!root.join("plugins/gears/__MACOSX").exists());
        // Installed means loaded: the package reads back and plugin.list shows it.
        assert!(read_package(&root.join("plugins/gears").to_string_lossy()).is_ok());
        let list = s.execute("plugin.list", &json!({})).unwrap();
        assert_eq!(list["plugins"][0]["commands"], json!(["gears.make"]));
        let only = std::fs::read_dir(root.join("plugins")).unwrap().count();
        assert_eq!(only, 1, "no staging folders are left behind");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_zip_replaced_after_review_is_refused_before_files_or_grants_are_written() {
        let (mut s, root) = session("swapped-after-review");
        let path = requested_package(&root);
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true})).unwrap();
        assert!(dry["approval"].as_str().is_some_and(|hash| !hash.is_empty()));
        // The same manifest with different component bytes must also invalidate the review.
        let mut replacement = unpack(&std::fs::read(&path).unwrap()).unwrap();
        replacement.files.iter_mut().find(|file| file.path == "plugin.wasm").unwrap().bytes = b"replaced component".to_vec();
        let files: Vec<_> = replacement.files.iter().map(|file| (file.path.as_str(), file.bytes.as_slice())).collect();
        std::fs::write(&path, zip(&files)).unwrap();
        let approve = json!({"command_groups":["file"],"folders":[{"name":"project","path":root,"read":true,"write":false}]});
        let error = s.execute("plugin.install", &json!({"path":path,"approval":dry["approval"],"approve":approve})).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::InvalidInput { field, .. }) if field == "approval"));
        assert!(!root.join("plugins").exists(), "even the plugins root and staging folders stay unwritten");
        assert!(s.plugin_state.grants.is_empty());
        assert!(s.plugin_state.packages.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    /// #20814: a write that finds another write in progress waits for it, so a job the app gave up on
    /// never overlaps the next install or removal.
    #[test]
    fn a_package_write_waits_for_the_write_in_progress() {
        let (_s, root) = session("write-lock");
        let plugin = unpack(&gears("0.1.0")).unwrap();
        let dir = root.join("plugins");
        let held = crate::plugin::install::write_lock();
        let (finished, done) = std::sync::mpsc::channel();
        let worker = std::thread::spawn({
            let dir = dir.clone();
            move || {
                let result = crate::plugin::install::install(&plugin, &dir, false);
                let _ = finished.send(());
                result
            }
        });
        assert!(done.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "the second write started while the first held the lock");
        assert!(!dir.exists(), "nothing was written yet");
        drop(held);
        assert!(done.recv_timeout(std::time::Duration::from_secs(60)).is_ok(), "the waiting write runs once the lock is free");
        assert!(worker.join().unwrap().is_ok());
        assert!(dir.join("gears").join("plugin.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn replacing_an_installed_plugin_needs_replace() {
        let (mut s, root) = session("replace");
        s.execute("plugin.install", &json!({"path": put(&root, "a.zip", &gears("0.1.0"))})).unwrap();
        let newer = put(&root, "b.zip", &gears("0.2.0"));
        let dry = s.execute("plugin.install", &json!({"path": newer, "dry_run": true})).unwrap();
        assert_eq!(dry["replaces"], "Gears 0.1.0");
        let refused = s.execute("plugin.install", &json!({"path": newer})).unwrap_err().to_string();
        assert!(refused.contains("already installed"), "{refused}");
        let dir = root.join("plugins");
        let error = crate::plugin::install::install(&unpack(&gears("0.2.0")).unwrap(), &dir, false).unwrap_err();
        assert_eq!(error, InstallError::AlreadyInstalled { id: "gears".into(), name: "Gears".into(), version: "0.1.0".into() });
        assert_eq!(read_package(&dir.join("gears").to_string_lossy()).unwrap().manifest.version, "0.1.0", "kept as it was");
        s.execute("plugin.install", &json!({"path": newer, "replace": true})).unwrap();
        assert_eq!(read_package(&dir.join("gears").to_string_lossy()).unwrap().manifest.version, "0.2.0");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "the old folder is gone");
        let _ = std::fs::remove_dir_all(root);
    }

    fn requested_package(root: &Path) -> String {
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"command_groups":["app","file"],"files":[{"name":"project","read":true,"write":true}]});
        m["runtime"]["entries"] = json!([{"name":"make","slots":[{"kind":"menu","name":"File"},{"kind":"ribbon","name":"Model"}]}]);
        put(root, "requested.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"inert bytes")]))
    }

    #[test]
    fn install_approvals_are_validated_before_writing_and_dry_run_never_stores_them() {
        let (mut s, root) = session("approval");
        let path = requested_package(&root);
        let approve = json!({"command_groups":["file"],"folders":[{"name":"project","path":root,"read":true,"write":false}]});
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true,"approve":approve})).unwrap();
        assert_eq!(dry["entry_points"][0]["slots"].as_array().unwrap().len(), 2);
        assert!(s.plugin_state.grants.is_empty() && !root.join("plugins").exists());
        for unasked in [
            json!({"command_groups":["plugin"]}),
            json!({"folders":[{"name":"unasked","path":root,"read":true,"write":false}]}),
            json!({"folders":[{"name":"project","path":"relative","read":true,"write":false}]}),
            json!({"folders":[{"name":"project","path":root,"read":false,"write":false}]}),
            json!({"folders":[{"name":"project","path":root,"read":true,"write":false},{"name":"project","path":root,"read":true,"write":false}]}),
        ] {
            for dry_run in [false, true] {
                let error = s.execute("plugin.install", &json!({"path":path,"dry_run":dry_run,"approve":unasked})).unwrap_err();
                assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { .. })), "{error}");
                assert!(s.plugin_state.grants.is_empty() && !root.join("plugins").exists());
            }
        }
        // Reject a write escalation even when the folder's name is requested.
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":false}]});
        let read_only = put(&root, "read-only.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"x")]));
        let error = s
            .execute("plugin.install", &json!({"path":read_only,"approve":{"folders":[{"name":"project","path":root,"read":true,"write":true}]}}))
            .unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::PermissionDenied { .. })));
        s.execute("plugin.install", &json!({"path":path,"approve":approve})).unwrap();
        let granted = s.plugin_state.grants["sample"].clone();
        assert_eq!(granted.package_hash.as_deref(), Some(read_package(&root.join("plugins/sample").to_string_lossy()).unwrap().approval.as_str()));
        assert_eq!(granted.command_groups, BTreeSet::from(["file".into()]));
        assert_eq!(granted.folders, vec![FileGrant { name: "project".into(), folder: root.clone(), read: true, write: false }]);
        // A refused replacement cannot discard the original grants or package.
        assert!(s.execute("plugin.install", &json!({"path":path})).is_err());
        assert_eq!(s.plugin_state.grants["sample"], granted);
        assert!(s.execute("plugin.remove", &json!({"id":"../outside"})).is_err());
        s.disabled_plugins.insert("sample".into());
        let removed = s.execute("plugin.remove", &json!({"id":"sample"})).unwrap();
        assert_eq!(removed["removed"], true);
        assert!(!root.join("plugins/sample").exists());
        assert!(
            !s.plugin_state.grants.contains_key("sample")
                && !s.plugin_state.packages.contains_key("sample")
                && !s.disabled_plugins.contains("sample")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn approved_memory_raises_the_ceiling_for_that_exact_package_only() {
        let (mut s, root) = session("memory");
        let mut m = manifest();
        m["runtime"]["limits"]["memory_mib"] = json!(1024);
        let path = put(&root, "memory.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"inert bytes")]));
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true})).unwrap();
        assert_eq!((dry["limits"]["memory_mib"].as_u64(), dry["host_limits"]["max_memory_mib"].as_u64()), (Some(1024), Some(128)));
        for wrong in [json!(512), json!(2048), json!(0), json!(-1), json!("1024")] {
            assert!(s.execute("plugin.install", &json!({"path":path,"approve":{"memory_mib":wrong}})).is_err(), "{wrong}");
            assert!(s.plugin_state.grants.is_empty() && !root.join("plugins").exists());
        }
        s.execute("plugin.install", &json!({"path":path,"approve":{"memory_mib":1024}})).unwrap();
        let approval = read_package(&root.join("plugins/sample").to_string_lossy()).unwrap().approval;
        assert_eq!(s.plugin_state.grants["sample"].memory_mib, Some(1024));
        assert_eq!(host_limits_for(&s, "sample", &approval).max_memory_mib, 1024);
        assert_eq!(host_limits_for(&s, "sample", "another package").max_memory_mib, 128);
        assert_eq!(s.plugin_limits.max_memory_mib, 128, "the user's ceiling for other plugins is unchanged");
        let inspected = s.execute("plugin.inspect", &json!({"path":root.join("plugins/sample")})).unwrap();
        assert_eq!(inspected["within_host_limits"], true);
        // Reinstalling without approving memory takes the grant away.
        s.execute("plugin.install", &json!({"path":path,"replace":true})).unwrap();
        assert_eq!(host_limits_for(&s, "sample", &approval).max_memory_mib, 128);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn approved_fuel_is_stored_for_the_exact_package_and_replacement_asks_again() {
        let (mut s, root) = session("fuel");
        let fuel = 10_000_000_000_000_u64;
        let mut m = manifest();
        m["runtime"]["limits"]["fuel"] = json!(fuel);
        let path = put(&root, "fuel.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"inert bytes")]));
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true})).unwrap();
        assert_eq!(dry["limits"]["fuel"], fuel);
        assert_eq!(dry["host_limits"]["max_fuel"], 100_000_000_000_u64);
        assert!(s.plugin_state.grants.is_empty());
        for wrong in [json!(fuel - 1), json!(fuel + 1), json!(0), json!(-1), json!("10000000000000")] {
            assert!(s.execute("plugin.install", &json!({"path":path,"approve":{"fuel":wrong}})).is_err(), "{wrong}");
            assert!(!root.join("plugins").exists());
        }
        s.plugin_state.stored = Some(Default::default());
        s.execute("plugin.install", &json!({"path":path,"approval":dry["approval"],"approve":{"fuel":fuel}})).unwrap();
        let installed = root.join("plugins/sample");
        let approval = read_package(&installed.to_string_lossy()).unwrap().approval;
        assert_eq!(serde_json::to_value(&s.plugin_state.grants["sample"]).unwrap()["fuel"], fuel);
        assert_eq!(host_limits_for(&s, "sample", &approval).max_fuel, fuel);
        assert_eq!(host_limits_for(&s, "other", &approval).max_fuel, s.plugin_limits.max_fuel);
        assert_eq!(s.plugin_limits.max_fuel, 100_000_000_000);
        let mut later = Session::default();
        later.plugin_state.plugins_dir = Some(root.join("plugins"));
        later.plugin_state.stored = Some(Default::default());
        later.plugin_state.refresh_stored();
        assert_eq!(later.execute("plugin.grants", &json!({})).unwrap()["grants"][0]["fuel"], fuel);
        assert_eq!(host_limits_for(&later, "sample", &approval).max_fuel, fuel);
        m["runtime"]["limits"]["fuel"] = json!(fuel - 1);
        std::fs::write(installed.join("plugin.json"), m.to_string()).unwrap();
        let changed = read_package(&installed.to_string_lossy()).unwrap().approval;
        assert_ne!(approval, changed);
        assert_eq!(host_limits_for(&later, "sample", &changed).max_fuel, later.plugin_limits.max_fuel);
        assert_eq!(later.execute("plugin.grants", &json!({})).unwrap()["grants"][0]["status"], "changed");
        s.execute("plugin.install", &json!({"path":path,"replace":true})).unwrap();
        assert_eq!(host_limits_for(&s, "sample", &approval).max_fuel, s.plugin_limits.max_fuel);
        s.execute("plugin.revoke", &json!({"id":"sample"})).unwrap();
        assert!(s.execute("plugin.grants", &json!({})).unwrap()["grants"].as_array().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fuel_above_the_hard_ceiling_is_refused_before_installation() {
        let (mut s, root) = session("fuel-hard-ceiling");
        let mut m = manifest();
        let ceiling = 10_000_000_000_000_u64;
        m["runtime"]["limits"]["fuel"] = json!(ceiling + 1);
        let path = put(&root, "fuel.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"inert bytes")]));
        s.plugin_limits.max_fuel = u64::MAX;
        for args in [json!({"path":path,"dry_run":true}), json!({"path":path,"approve":{"fuel":ceiling + 1}})] {
            let error = s.execute("plugin.install", &args).unwrap_err();
            assert_eq!(error.host_error(), Some(&HostError::OverCeiling { limit: "fuel".into(), requested: ceiling + 1, allowed: ceiling }));
            assert!(error.to_string().contains(&(ceiling + 1).to_string()) && error.to_string().contains(&ceiling.to_string()));
            assert!(!root.join("plugins").exists() && s.plugin_state.grants.is_empty());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installs_without_approval_grant_nothing_and_replacement_drops_stale_grants() {
        let (mut s, root) = session("stale");
        let path = requested_package(&root);
        s.execute("plugin.install", &json!({"path":path})).unwrap();
        assert_eq!(s.plugin_state.grants["sample"].command_groups, BTreeSet::new());
        assert!(s.plugin_state.grants["sample"].folders.is_empty());
        assert!(s.plugin_state.grants["sample"].package_hash.is_some());
        s.execute("plugin.install", &json!({"path":path,"replace":true,"approve":{"command_groups":["app","file"],"folders":[{"name":"project","path":root,"read":true,"write":true}]}})).unwrap();
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"command_groups":["app"]});
        let newer = put(&root, "newer.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"new bytes")]));
        s.execute("plugin.install", &json!({"path":newer,"replace":true,"approve":{"command_groups":["app"]}})).unwrap();
        let package_hash = Some(read_package(&root.join("plugins/sample").to_string_lossy()).unwrap().approval);
        assert_eq!(
            s.plugin_state.grants["sample"],
            Grants { package_hash: package_hash.clone(), command_groups: BTreeSet::from(["app".into()]), ..Default::default() }
        );
        s.execute("plugin.install", &json!({"path":newer,"replace":true})).unwrap();
        assert_eq!(s.plugin_state.grants["sample"], Grants { package_hash, ..Default::default() });
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hostile_install_approvals_and_remove_scopes_leave_the_package_unchanged() {
        let (mut s, root) = session("hostile");
        let path = requested_package(&root);
        for approve in [
            json!(false),
            json!([]),
            json!([["file"], []]),
            json!({"folders":[["project",root,true,true]]}),
            json!({"extra":true}),
            json!({"folders":[{"name":"project","folder":root,"read":true,"write":true}]}),
            json!({"folders":[{"name":"project","path":root,"read":"yes","write":true}]}),
        ] {
            assert!(
                s.execute("plugin.install", &json!({"path":path,"approve":approve})).is_err(),
                "refuse non-object or invalid approval: {approve}"
            );
            assert!(!root.join("plugins").exists());
        }
        let approve = json!({"folders":[{"name":"project","path":root,"read":true,"write":true}]});
        assert!(s.execute("plugin.install", &json!([path, false, false, approve])).is_err(), "install parameters must be an object");
        assert!(!root.join("plugins").exists());
        solidcraft_io::vfs::scoped(Some(Default::default()), || {
            assert!(s.execute("plugin.install", &json!({"path":path,"approve":approve})).is_err());
        });
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.join("zips")])), || {
            assert!(s.execute("plugin.install", &json!({"path":path,"approve":approve})).is_err(), "folder approval retains caller scope");
        });
        assert!(!root.join("plugins").exists());
        s.execute("plugin.install", &json!({"path":path,"approve":approve})).unwrap();
        let hidden = root.join("plugins/sample/.private");
        std::fs::write(&hidden, "private").unwrap();
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::default().with_design_folder(&root)), || {
            assert!(s.execute("plugin.remove", &json!({"id":"sample"})).is_err());
        });
        // Replacement must not erase hidden descendants that this caller cannot remove.
        let prior_grants = s.plugin_state.grants["sample"].clone();
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::default().with_design_folder(&root)), || {
            assert!(s.execute("plugin.install", &json!({"path":path,"replace":true})).is_err());
        });
        assert_eq!(s.plugin_state.grants["sample"], prior_grants);
        assert!(hidden.exists() && s.plugin_state.grants.contains_key("sample"));
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.join("plugins")])), || {
            s.execute("plugin.remove", &json!({"id":"sample"})).unwrap();
        });
        let hidden_zip = put(
            &root,
            "hidden.zip",
            &zip(&[("plugin.json", &serde_json::to_vec(&manifest()).unwrap()), ("plugin.wasm", b"x"), (".private", b"new private data")]),
        );
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::default().with_design_folder(&root)), || {
            assert!(s.execute("plugin.install", &json!({"path":hidden_zip})).is_err());
        });
        assert!(!root.join("plugins/sample").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_oversized_and_incomplete_zips_are_refused_before_anything_is_written() {
        let good_bytes = manifest_bytes("gears", json!(["commands@0.1"]));
        let good: &[u8] = &good_bytes;
        let slip = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", b"x"), ("../evil", b"x")])).unwrap_err();
        assert!(matches!(slip, InstallError::Archive(safe_zip::Error::UnsafeEntry { .. })), "{slip}");
        let absolute = unpack(&zip(&[("/plugin.json", good)])).unwrap_err();
        assert!(matches!(absolute, InstallError::Archive(safe_zip::Error::UnsafeEntry { .. })), "{absolute}");
        let huge = vec![0u8; craft_plugin::MAX_IMPORTER_COMPONENT_BYTES + 1];
        let bomb = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", &huge[..])])).unwrap_err();
        assert!(matches!(bomb, InstallError::Archive(safe_zip::Error::EntryTooLarge { .. })), "{bomb}");
        // A command plugin's component stays within 16 MiB; only an importer's may be larger.
        let big = vec![0u8; MAX_COMPONENT_BYTES + 1];
        let command = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", &big[..])])).unwrap_err();
        assert!(matches!(command, InstallError::Manifest(HostError::LimitExceeded { .. })), "{command}");
        let chunk = vec![0u8; 15 << 20];
        let many: Vec<String> = (0..6).map(|i| format!("data/{i}.bin")).collect();
        let mut files: Vec<(&str, &[u8])> = vec![("plugin.json", good), ("plugin.wasm", b"x")];
        files.extend(many.iter().map(|n| (n.as_str(), chunk.as_slice())));
        assert_eq!(unpack(&zip(&files)).unwrap_err(), InstallError::Archive(safe_zip::Error::TooLarge { limit: 80 << 20 }));
        assert_eq!(unpack(&zip(&[("plugin.wasm", b"x")])).unwrap_err(), InstallError::MissingManifest);
        assert_eq!(unpack(&zip(&[("plugin.json", good)])).unwrap_err(), InstallError::MissingModule("plugin.wasm".into()));
        // GitHub's "Source code" archive of a release: one folder with the manifest and the
        // sources, but no built module. The refusal points to the plugin's own .zip (#16736).
        let source = zip(&[("gears-0.1.1/plugin.json", good), ("gears-0.1.1/Cargo.toml", b"[package]"), ("gears-0.1.1/src/lib.rs", b"")]);
        let error = unpack(&source).unwrap_err();
        assert_eq!(error, InstallError::SourceOnly("plugin.wasm".into()));
        assert!(error.to_string().contains("the plugin's own .zip from its release"), "{error}");
        let two = zip(&[("a/plugin.json", good), ("a/plugin.wasm", b"x"), ("b/plugin.json", good), ("b/plugin.wasm", b"x")]);
        assert_eq!(unpack(&two).unwrap_err(), InstallError::NotOnePlugin);
        let stray = zip(&[("a/plugin.json", good), ("a/plugin.wasm", b"x"), ("other.txt", b"x")]);
        assert_eq!(unpack(&stray).unwrap_err(), InstallError::NotOnePlugin);
        let deep = zip(&[("a/b/plugin.json", good), ("a/b/plugin.wasm", b"x")]);
        assert_eq!(unpack(&deep).unwrap_err(), InstallError::NotOnePlugin);
        let newer = unpack(&zip(&[("plugin.json", &manifest_bytes("gears", json!(["commands@0.3"]))[..]), ("plugin.wasm", b"x")])).unwrap_err();
        assert!(matches!(&newer, InstallError::Manifest(HostError::UnsupportedCapability { .. })), "{newer}");
        assert_eq!(newer.to_string(), "needs commands 0.3; update SolidCraft or the plugin");
        // Through the command: refused, and the plugins folder is never created.
        let (mut s, root) = session("refused");
        let path = put(&root, "slip.zip", &zip(&[("plugin.json", good), ("plugin.wasm", b"x"), ("../evil", b"x")]));
        assert!(s.execute("plugin.install", &json!({"path": path})).is_err());
        let path = put(&root, "newer.zip", &zip(&[("plugin.json", &manifest_bytes("gears", json!(["commands@0.3"]))[..]), ("plugin.wasm", b"x")]));
        let error = s.execute("plugin.install", &json!({"path": path})).unwrap_err();
        assert!(matches!(error.host_error(), Some(HostError::UnsupportedCapability { .. })), "{error}");
        assert!(!root.join("plugins").exists() && !root.join("evil").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}

/// The `storage.*` commands as a running plugin sees them: its own data only, files in parts, a
/// merge patch, and the size limits.
#[test]
fn storage_commands_reach_only_the_running_plugins_data_within_its_limits() {
    let mut s = Session::default();
    let other = br#"{"kept":true}"#;
    s.doc_mut().plugin_data.insert("other-tool/data.json".into(), other.into());
    s.plugin_state.storage_caller =
        Some(storage::StorageCaller { id: "gears".into(), name: "Gears".into(), version: "1.4.0".into(), data_format: 3 });
    s.execute("storage.set", &json!({"document":{"teeth":24,"module":{"value":2,"unit":"mm"}}})).unwrap();
    s.execute("storage.patch", &json!({"patch":{"teeth":30,"module":{"unit":null},"name":"spur"}})).unwrap();
    let data = s.execute("storage.get", &json!({})).unwrap();
    assert_eq!(data["document"], json!({"teeth":30,"module":{"value":2},"name":"spur"}));
    assert_eq!((data["format"].clone(), data["plugin_format"].clone(), data["read_only"].clone()), (json!(3), json!(3), json!(false)));
    let meta = solidcraft_doc::plugin_data::meta(&s.doc.plugin_data, "gears");
    assert_eq!((meta.name.as_str(), meta.plugin_version.as_str(), meta.format), ("Gears", "1.4.0", 3));
    // A plugin reads only its own data, and never changes another's.
    assert!(s.execute("storage.get", &json!({"plugin":"other-tool"})).unwrap_err().to_string().contains("only its own data"));
    assert_eq!(s.doc.plugin_data["other-tool/data.json"].as_bytes(), other);

    // A file written in parts reads back in parts, as bytes; writing it as a cache file moves it.
    assert_eq!(storage::write_blob(&mut s, "paths/op3.bin", &[1, 2, 3], false, false).unwrap(), 3);
    assert_eq!(storage::write_blob(&mut s, "paths/op3.bin", &[4, 5], false, true).unwrap(), 5);
    let part = storage::read_blob(&s, None, "paths/op3.bin", 3, 10).unwrap();
    assert_eq!(part, storage::BlobBytes { data: vec![4, 5], cache: false, bytes: 5 });
    assert!(storage::read_blob(&s, None, "paths/op3.bin", 99, 10).unwrap().data.is_empty(), "past the end");
    storage::write_blob(&mut s, "paths/op3.bin", &[9], true, false).unwrap();
    let blobs = s.execute("storage.get", &json!({})).unwrap()["blobs"].clone();
    assert_eq!(blobs, json!([{"name":"paths/op3.bin","cache":true,"bytes":1}]));
    assert!(storage::write_blob(&mut s, "../escape", &[], false, false).is_err());
    assert!(storage::read_blob(&s, None, "missing.bin", 0, 10).unwrap_err().to_string().contains("keeps no file named"));
    // The base64 commands are gone: a file's bytes are never text.
    for retired in ["storage.put_blob", "storage.get_blob"] {
        assert!(crate::find_command(retired).is_none(), "{retired}");
    }
    assert_eq!(s.execute("storage.delete_blob", &json!({"name":"paths/op3.bin"})).unwrap()["deleted"], true);
    assert_eq!(s.execute("storage.delete_blob", &json!({"name":"paths/op3.bin"})).unwrap()["deleted"], false);
    // Over the main document's limit, nothing changes.
    let before = s.doc.plugin_data.clone();
    let big = "x".repeat(storage::MAX_DOCUMENT_BYTES);
    assert!(s.execute("storage.set", &json!({"document":big})).unwrap_err().to_string().contains("16 MiB"));
    assert_eq!(s.doc.plugin_data, before);
    // Outside a plugin's run, reads name the plugin and writes are refused.
    s.plugin_state.storage_caller = None;
    assert!(s.execute("storage.get", &json!({})).is_err());
    assert_eq!(s.execute("storage.get", &json!({"plugin":"other-tool"})).unwrap()["document"], json!({"kept":true}));
    assert!(s.execute("storage.patch", &json!({"patch":{}})).is_err());
}

#[test]
fn storage_large_main_document_reads_in_bounded_parts() {
    let mut s = Session::default();
    s.plugin_state.storage_caller = Some(storage::StorageCaller { id: "gears".into(), name: "Gears".into(), version: "1".into(), data_format: 1 });
    assert_eq!(storage::read_document(&s, None, 0, 10).unwrap(), (Vec::new(), 0));
    let first = "x".repeat(600 * 1024);
    // Include multibyte JSON text: parts are bytes, not independently parsed JSON strings.
    let second = "é".repeat(300 * 1024);
    s.execute("storage.patch", &json!({"patch":{"first":first}})).unwrap();
    s.execute("storage.patch", &json!({"patch":{"second":second}})).unwrap();
    let expected = json!({"first":first,"second":second});
    assert!(serde_json::to_vec(&expected).unwrap().len() > 1 << 20);
    let mut joined = Vec::new();
    loop {
        let (part, bytes) = storage::read_document(&s, None, joined.len() as u64, storage::MAX_READ_BYTES).unwrap();
        assert!(!part.is_empty() && part.len() <= storage::MAX_READ_BYTES as usize);
        joined.extend_from_slice(&part);
        if joined.len() as u64 == bytes {
            break;
        }
    }
    assert_eq!(serde_json::from_slice::<Value>(&joined).unwrap(), expected);
    assert_eq!(storage::read_document(&s, None, 0, 5).unwrap().0, joined[..5]);
    assert!(storage::read_document(&s, None, u64::MAX, 5).unwrap().0.is_empty());
    for len in [0, storage::MAX_READ_BYTES + 1] {
        assert!(storage::read_document(&s, None, 0, len).is_err());
    }
    assert!(storage::read_document(&s, Some("other-tool"), 0, 5).is_err());
    // The command takes no offset or length any more: its reply is the data itself.
    assert!(s.execute("storage.get", &json!({"offset":0})).is_err());
    s.plugin_state.storage_caller = None;
    assert!(storage::read_document(&s, None, 0, 5).is_err());
    assert_eq!(storage::read_document(&s, Some("gears"), 0, 5).unwrap().0, joined[..5]);
}

// Fill a quota with one shared chunk, keeping boundary tests small in memory.
fn fill_storage_bytes(s: &mut Session, bytes: u64) {
    let chunk = 1 << 20;
    let shared = solidcraft_doc::blob::Blob::from(vec![0; chunk]);
    for i in 0..bytes / chunk as u64 {
        s.doc_mut().plugin_data.insert(format!("filler/blobs/{i}"), shared.clone());
    }
    let tail = bytes as usize % chunk;
    if tail != 0 {
        s.doc_mut().plugin_data.insert("filler/blobs/tail".into(), vec![0; tail].into());
    }
}

#[test]
fn storage_append_preserves_bytes_when_changing_categories() {
    let mut s = Session::default();
    s.plugin_state.storage_caller = Some(storage::StorageCaller { id: "gears".into(), name: "Gears".into(), version: "1".into(), data_format: 1 });
    storage::write_blob(&mut s, "tool.bin", &[1, 2, 3], true, false).unwrap();
    for (cache, next, expected) in [(false, 4, vec![1, 2, 3, 4]), (true, 5, vec![1, 2, 3, 4, 5])] {
        storage::write_blob(&mut s, "tool.bin", &[next], cache, true).unwrap();
        let read = storage::read_blob(&s, None, "tool.bin", 0, storage::MAX_READ_BYTES).unwrap();
        assert_eq!((read.data, read.cache), (expected, cache));
        assert_eq!(s.doc.plugin_data.len(), 2);
    }
}

#[test]
fn storage_category_replacement_uses_final_file_and_byte_counts() {
    let mut s = Session::default();
    s.plugin_state.storage_caller = Some(storage::StorageCaller { id: "gears".into(), name: "Gears".into(), version: "1".into(), data_format: 1 });
    storage::write_blob(&mut s, "tool.bin", &[1, 2, 3], false, false).unwrap();
    for i in 0..solidcraft_doc::plugin_data::MAX_FILES - 2 {
        s.doc_mut().plugin_data.insert(format!("filler/blobs/{i}"), Default::default());
    }
    storage::write_blob(&mut s, "tool.bin", &[4], true, false).unwrap();
    assert_eq!(s.doc.plugin_data.len(), solidcraft_doc::plugin_data::MAX_FILES);
    assert_eq!(storage::blob(&s.doc, "gears", "cache/tool.bin"), Some(&[4][..]));
    let before = s.doc.plugin_data.clone();
    assert!(storage::write_blob(&mut s, "new.bin", &[], false, false).is_err());
    assert_eq!(s.doc.plugin_data, before);

    s.doc_mut().plugin_data.retain(|key, _| key.starts_with("gears/"));
    let meta_bytes = storage::blob(&s.doc, "gears", "meta.json").unwrap().len() as u64;
    fill_storage_bytes(&mut s, storage::MAX_TOTAL_BYTES - meta_bytes - 1);
    // The final total is exactly the limit even though the destination is new.
    storage::write_blob(&mut s, "tool.bin", &[5], false, false).unwrap();
    let before = s.doc.plugin_data.clone();
    assert!(storage::write_blob(&mut s, "tool.bin", &[6, 7], true, false).is_err());
    assert_eq!(s.doc.plugin_data, before);
    assert_eq!(storage::blob(&s.doc, "gears", "blobs/tool.bin"), Some(&[5][..]));
}

#[test]
fn storage_total_counts_metadata_creation_growth_and_shrinkage_before_mutation() {
    let mut s = Session::default();
    let mut caller = storage::StorageCaller { id: "gears".into(), name: "Gears".into(), version: "1".into(), data_format: 1 };
    fill_storage_bytes(&mut s, storage::MAX_TOTAL_BYTES - 1);
    let before = s.doc.plugin_data.clone();
    assert!(storage::put(&mut s, &caller, "blobs/tool.bin", Some(&[1])).is_err());
    assert_eq!(s.doc.plugin_data, before);

    s.doc_mut().plugin_data.clear();
    storage::put(&mut s, &caller, "blobs/tool.bin", Some(&[1])).unwrap();
    let meta_bytes = storage::blob(&s.doc, "gears", "meta.json").unwrap().len() as u64;
    fill_storage_bytes(&mut s, storage::MAX_TOTAL_BYTES - meta_bytes - 1);
    let before = s.doc.plugin_data.clone();
    caller.name.push('s');
    assert!(storage::put(&mut s, &caller, "blobs/tool.bin", Some(&[1])).is_err());
    assert_eq!(s.doc.plugin_data, before);
    // Even deleting a payload must account for a metadata increase larger than that payload.
    caller.name.push('s');
    assert!(storage::put(&mut s, &caller, "blobs/tool.bin", None).is_err());
    assert_eq!(s.doc.plugin_data, before);
    // A shrinking payload can pay for a growing metadata record.
    caller.name.pop();
    storage::put(&mut s, &caller, "blobs/tool.bin", None).unwrap();
    // A shrinking metadata record can pay for a growing payload.
    caller.name = "Gears".into();
    storage::put(&mut s, &caller, "blobs/tool.bin", Some(&[2])).unwrap();
}

/// The grant store the desktop app, the command line and headless MCP share.
#[cfg(not(target_arch = "wasm32"))]
mod stored_grants {
    use super::*;
    use crate::plugin::grant_store::{self, Synced};

    fn root(name: &str) -> PathBuf {
        let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-grant-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    fn approved(groups: &[&str]) -> Grants {
        Grants { package_hash: Some("a".repeat(64)), command_groups: groups.iter().map(|g| g.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn hostile_records_are_ignored_and_grant_nothing() {
        let plugins = root("hostile");
        grant_store::write(&plugins, "good", &approved(&["file"])).unwrap();
        let dir = grant_store::folder(&plugins);
        let record = |grants: Value| serde_json::to_vec(&grants).unwrap();
        for (name, bytes) in [
            ("broken.json", b"{".to_vec()),
            ("unknown.json", record(json!({"command_groups":["file"],"net":true}))),
            ("group.json", record(json!({"command_groups":["everything"]}))),
            ("memory.json", record(json!({"memory_mib":1_000_000}))),
            ("fuel.json", record(json!({"fuel":10_000_000_000_001_u64}))),
            ("zero-fuel.json", record(json!({"fuel":0}))),
            ("hash.json", record(json!({"package_hash":"not a hash"}))),
            ("relative.json", record(json!({"folders":[{"name":"out","folder":"out","read":true,"write":true}]}))),
            ("Bad_Id.json", record(json!({}))),
            ("large.json", vec![b' '; 70 << 10]),
            ("notes.txt", record(json!({}))),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        std::fs::create_dir(dir.join("folder.json")).unwrap();
        assert_eq!(grant_store::read_all(&plugins), BTreeMap::from([("good".to_string(), approved(&["file"]))]));
        assert!(grant_store::write(&plugins, "../evil", &approved(&[])).is_err() && grant_store::delete(&plugins, "../evil").is_err());
        assert!(grant_store::write(&plugins, "bad", &approved(&["everything"])).is_err());
        assert!(!plugins.join("evil.json").exists() && !dir.join("bad.json").exists());
        grant_store::delete(&plugins, "absent").unwrap();
        std::fs::remove_dir_all(plugins).unwrap();
    }

    #[test]
    fn a_store_outside_the_file_scope_is_neither_read_nor_written() {
        let (plugins, elsewhere) = (root("scope"), root("scope-elsewhere"));
        grant_store::write(&plugins, "gears", &approved(&["file"])).unwrap();
        let (mut synced, mut working) = (Synced::default(), BTreeMap::new());
        synced.refresh(&plugins, &mut working);
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([elsewhere.clone()])), || {
            assert!(grant_store::read_all(&plugins).is_empty());
            assert!(grant_store::write(&plugins, "bolts", &approved(&[])).is_err());
            assert!(grant_store::delete(&plugins, "gears").is_err());
            synced.refresh(&plugins, &mut working);
        });
        assert_eq!(working.keys().collect::<Vec<_>>(), ["gears"], "a store out of reach is no revocation");
        assert_eq!(grant_store::read_all(&plugins).keys().collect::<Vec<_>>(), ["gears"]);
        std::fs::remove_dir_all(plugins).unwrap();
        std::fs::remove_dir_all(elsewhere).unwrap();
    }

    #[test]
    fn each_host_writes_its_own_changes_and_takes_in_the_others() {
        let plugins = root("sync");
        let (mut desktop, mut cli) = (Synced::default(), Synced::default());
        let mut desktop_grants = BTreeMap::from([("gears".to_string(), approved(&["file"]))]);
        // No store yet: the desktop's grants move to it.
        desktop.adopt(&plugins, &mut desktop_grants).unwrap();
        assert_eq!(grant_store::read_all(&plugins), desktop_grants);
        let mut cli_grants = BTreeMap::new();
        cli.refresh(&plugins, &mut cli_grants);
        assert_eq!(cli_grants, desktop_grants);
        // The command line approves another plugin and revokes the desktop's.
        cli_grants.insert("bolts".into(), approved(&["doc"]));
        cli_grants.remove("gears");
        cli.persist(&plugins, &cli_grants).unwrap();
        // The desktop changed nothing of either meanwhile, so it takes both in.
        desktop_grants.insert("nuts".into(), approved(&[]));
        desktop.persist(&plugins, &desktop_grants).unwrap();
        desktop.refresh(&plugins, &mut desktop_grants);
        assert_eq!(desktop_grants.keys().collect::<Vec<_>>(), ["bolts", "nuts"]);
        assert_eq!(grant_store::read_all(&plugins), desktop_grants);
        // Once the store exists it is the whole truth: an old desktop record is not brought back.
        let mut restarted = BTreeMap::from([("gears".to_string(), approved(&["file"]))]);
        Synced::default().adopt(&plugins, &mut restarted).unwrap();
        assert_eq!(restarted, desktop_grants);
        std::fs::remove_dir_all(plugins).unwrap();
    }
}
