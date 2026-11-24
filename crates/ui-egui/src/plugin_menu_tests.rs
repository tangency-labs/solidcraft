//! The Plugins menu and its entry dialog: schema fields, selection fields picked in the viewport,
//! required gating, and a run through the dialog matching `plugin.run`.
use super::*;
use std::collections::BTreeSet;

/// Wait on the validator's channel, without timing or driving the design. A cancelled old
/// request can finish first; the next pass then validates the current inputs.
fn checked_prefill(app: &SolveApp) {
    for dialog in app.plugin_menu.dialogs.iter().filter(|d| d.form_guard.is_some()) {
        for _ in 0..3 {
            let _ = dialog.field_errors(app);
            let running = dialog.validation.lock().unwrap().running.take();
            let Some(running) = running else { break };
            let errors = running.reply.recv().expect("validation worker finished");
            if !running.progress.is_cancelled() {
                dialog.validation.lock().unwrap().cached = Some(CachedValidation { key: running.key.clone(), errors });
            }
        }
        assert!(dialog.validation.lock().unwrap().running.is_none());
    }
}

/// Inspect UI shapes without uploading textures; clear those deltas before any assertion
/// can unwind, as required by the egui test backend.
fn form_frame(ctx: &egui::Context, paint: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
    let mut output = ctx.run_ui(Default::default(), paint);
    output.textures_delta.clear();
    output
}

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
fn edit_form_prefills_values_picks_conditions_and_draws_field_errors() {
    let mut h = box_and_pick_plugin();
    h.app.home.open = false;
    let before = h.app.session.doc.clone();
    open_form(&mut h.app, "inserts", "seats", &json!({"edges":[{"type":"edge","body":"gone","index":0,"point":[0,0,0]}],"body":[]})).unwrap();
    checked_prefill(&h.app);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert!(!dialog.preview, "opening a populated form runs no entry until Apply or an explicit Preview request");
    assert_eq!(picks(&h)[0], vec![Sel::Edge { body: "gone".into(), index: 0, point: solidcore_record::Vec3::ZERO }]);
    assert!(dialog.ready(&h.app).is_err());
    h.frames(2);
    assert_eq!(h.app.session.doc, before, "prefill and preview never commit");
    h.app.plugin_menu.dialogs.clear();
    crate::dialogs::cancel(&mut h.app);
    assert_eq!(h.app.session.doc, before, "Cancel leaves the design equal");

    let mut command = entry("cabinet", "edit", json!([]));
    command.input_schema = cabinet_schema();
    command.input_schema["properties"]["operation_id"] = json!({"type":"string"});
    command.input_schema["additionalProperties"] = json!(true);
    h.app.plugin_menu.listed = vec![listed("cabinet", vec![command])];
    open_form(
        &mut h.app,
        "cabinet",
        "cabinet",
        &json!({"storage":"drawers","drawer_count":9.0,"length":42,"size":"large","operation_id":"","saved_metadata":{"source":"stored"}}),
    )
    .unwrap();
    let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
    assert_eq!(params(fields).unwrap()["drawer_count"], 9);
    assert_eq!(
        fields.iter().find(|(f, _)| f.key == "drawer_count").unwrap().1,
        Input::Integer(9),
        "schema-valid integral JSON floats prefill integer fields"
    );
    assert_eq!(params(fields).unwrap()["length"], 42.0);
    assert_eq!(params(fields).unwrap()["operation_id"], "", "explicit empty values survive prefill");
    assert!(params(fields).unwrap().get("shelf_count").is_none());
    checked_prefill(&h.app);
    assert_eq!(
        h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap()["saved_metadata"],
        json!({"source":"stored"}),
        "schema-allowed extra values survive Apply"
    );
}

#[test]
fn prefilled_validation_runs_off_thread_and_discards_changed_inputs() {
    let mut h = box_and_pick_plugin();
    open_form(&mut h.app, "inserts", "seats", &json!({"depth":2,"edges":[],"body":[]})).unwrap();
    let before = h.app.session.doc.clone();
    let (release, wait) = std::sync::mpsc::channel();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.validation_gate = Some(Arc::new(std::sync::Mutex::new(wait)));
    dialog.preview = false;
    // This returns while the worker waits on the channel: the window can edit or cancel.
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap_err(), h.app.tr("ui.plugin_edit_checking"));
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "depth").unwrap().1 = Input::Number(9.0);
    dialog.validation_gate = None;
    release.send(()).unwrap();
    checked_prefill(&h.app);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    let values = dialog.validation.lock().unwrap().cached.as_ref().unwrap().key.values.clone();
    assert_eq!(values["depth"], 9.0, "the old validation result is discarded");
    assert_eq!(h.app.session.doc, before);
    // A cancelled reply can arrive after inputs return to its original key. It must still
    // be discarded, rather than caching its cancellation as an error for the current inputs.
    let key = dialog.validation.lock().unwrap().cached.as_ref().unwrap().key.clone();
    let progress = Arc::new(solidcraft_engine::doc::progress::Progress::default());
    progress.cancel();
    let (reply, received) = std::sync::mpsc::channel();
    reply.send(validation_failed()).unwrap();
    {
        let mut validation = dialog.validation.lock().unwrap();
        validation.cached = None;
        validation.running = Some(Checking { key, progress, reply: received });
    }
    checked_prefill(&h.app);
    assert!(!h.app.plugin_menu.dialog().unwrap().field_errors(&h.app).contains_key("params"));
    let (release, wait) = std::sync::mpsc::channel();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "depth").unwrap().1 = Input::Number(10.0);
    dialog.validation_gate = Some(Arc::new(std::sync::Mutex::new(wait)));
    assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_err());
    let progress = h.app.plugin_menu.dialog().unwrap().validation.lock().unwrap().running.as_ref().unwrap().progress.clone();
    h.app.plugin_menu.dialogs.clear();
    assert!(progress.is_cancelled(), "Cancel stops in-flight validation");
    release.send(()).unwrap();
    assert_eq!(h.app.session.doc, before, "Cancel commits no validation work");
}

#[test]
fn preflight_omits_absent_optional_selections_but_keeps_explicit_empty_values() {
    let mut h = box_and_pick_plugin();
    let schema = &mut h.app.plugin_menu.listed[0].entries[0].input_schema;
    schema["required"] = json!([]);
    schema["properties"]["edges"]["minItems"] = json!(1);
    open_form(&mut h.app, "inserts", "seats", &json!({"depth":2})).unwrap();
    checked_prefill(&h.app);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert!(dialog.ready(&h.app).is_ok(), "{:?}", dialog.field_errors(&h.app));
    assert!(dialog.ready(&h.app).unwrap().get("edges").is_none());
    h.app.plugin_menu.dialogs.clear();
    crate::dialogs::cancel(&mut h.app);
    open_form(&mut h.app, "inserts", "seats", &json!({"depth":2,"edges":[]})).unwrap();
    checked_prefill(&h.app);
    assert!(h.app.plugin_menu.dialog().unwrap().field_errors(&h.app).contains_key("edges"));
}

#[test]
fn validation_worker_disconnect_retries_and_field_errors_stay_cached() {
    let mut h = box_and_pick_plugin();
    open_form(&mut h.app, "inserts", "seats", &json!({"depth":2})).unwrap();
    checked_prefill(&h.app);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    let key = dialog.validation.lock().unwrap().cached.as_ref().unwrap().key.clone();
    let (sender, reply) = std::sync::mpsc::channel();
    drop(sender);
    {
        let mut validation = dialog.validation.lock().unwrap();
        validation.cached = None;
        validation.running = Some(Checking { key: key.clone(), progress: Default::default(), reply });
    }
    assert_eq!(dialog.field_errors(&h.app)["params"], h.app.tr("ui.plugin_start_failed"));
    checked_prefill(&h.app);
    assert!(!dialog.field_errors(&h.app).contains_key("params"), "a stopped worker must be retryable without editing inputs");
    let errors = [("edges".into(), solidcraft_engine::schema::plugin::FormFieldError::MissingPick)].into();
    dialog.validation.lock().unwrap().cached = Some(CachedValidation { key, errors });
    assert!(dialog.field_errors(&h.app).contains_key("edges"));
    assert!(dialog.validation.lock().unwrap().running.is_none(), "actual field errors stay cached");
}

fn deliver_form_reply(app: &mut SolveApp, value: Value) {
    let (request, reply) = solidcraft_control::ControlRequest::new("engine.execute", json!({}));
    request.reply.send(value).unwrap();
    app.plugin_menu.dialogs.last_mut().unwrap().running = Some(Running { serial: request.serial, reply, ticket: request.ticket, received: None });
}

#[test]
fn validation_reply_records_the_checked_revision() {
    let mut h = box_and_pick_plugin();
    open_form(&mut h.app, "inserts", "seats", &json!({"depth":2})).unwrap();
    let revision = h.app.session.design_revision();
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1})).unwrap();
    assert_ne!(revision, h.app.session.design_revision());
    deliver_form_reply(&mut h.app, json!({"ok":true,"result":{"form_validation":{"revision":revision,"changed":true}}}));
    poll(&mut h.app);
    assert_eq!(h.app.plugin_menu.dialog().unwrap().form_guard.as_ref().unwrap().revision, revision);
}

#[test]
fn completed_form_replies_wait_for_the_originating_session() {
    for kind in ["read", "edit", "validation"] {
        let mut h = box_and_pick_plugin();
        open_form(&mut h.app, "inserts", "seats", &json!({"depth":2})).unwrap();
        let session = h.app.session.uid;
        let revision = h.app.session.design_revision();
        h.app.plugin_menu.dialogs.last_mut().unwrap().read = kind == "read";
        let value = if kind == "validation" {
            json!({"ok":true,"result":{"form_validation":{"revision":revision,"changed":true}}})
        } else {
            json!({"ok":true,"result":{"result":{},"forms":[{"plugin":"inserts","entry":"seats","params":{"depth":7}}]}})
        };
        deliver_form_reply(&mut h.app, value);
        crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
        let other = h.app.session.doc.clone();
        poll(&mut h.app);
        assert_eq!(h.app.plugin_menu.dialogs.len(), 1, "{kind}: no forms on the other tab");
        let dialog = h.app.plugin_menu.dialog().unwrap();
        assert_eq!(dialog.session, session);
        assert!(dialog.running.is_some(), "{kind}: keep the reply until its design is active");
        assert_eq!(dialog.form_guard.as_ref().unwrap().revision, revision);
        assert_eq!(h.app.session.doc, other);
        crate::documents::switch(&mut h.app, 0);
        poll(&mut h.app);
        let dialog = h.app.plugin_menu.dialog().unwrap();
        assert!(dialog.running.is_none());
        assert_eq!(dialog.session, session);
        if kind != "validation" {
            assert_eq!(dialog.fields.as_ref().unwrap().iter().find(|(field, _)| field.key == "depth").unwrap().1, Input::Number(7.0));
        }
    }
}

#[test]
fn corrected_raw_selection_preserves_picker_field_identity() {
    let mut h = box_and_pick_plugin();
    h.app.plugin_menu.listed[0].entries[0].input_schema["properties"]["edges"]["minItems"] = json!(0);
    let name = h.app.session.model.state().bodies.first().unwrap().name.clone();
    let body = json!({"type":"body","name":name});
    open_form(&mut h.app, "inserts", "seats", &json!({"edges":"malformed","body":[body]})).unwrap();
    let mut dialog = h.app.plugin_menu.dialogs.pop().unwrap();
    let picker = h.app.dialog.as_ref().unwrap();
    assert_eq!(picker.inputs.len(), 1);
    let fields = dialog.fields.as_mut().unwrap();
    let (field, input) = fields.iter_mut().find(|(f, _)| f.key == "edges").unwrap();
    *input = stored_input(field, &json!([])).unwrap();
    take_picks(&mut dialog, picker, &h.app.session.doc);
    let values = params(dialog.fields.as_ref().unwrap()).unwrap();
    assert_eq!(values["edges"], json!([]));
    assert_eq!(values["body"], json!([body]));
    h.app.dialog.as_mut().unwrap().listed = Some(0);
    h.app.plugin_menu.dialogs.push(dialog);
    h.frames(2);
    let picker = h.app.dialog.as_ref().unwrap();
    assert!(picker.inputs[0].items.is_empty());
    assert_eq!(picker.inputs[1].items, vec![Sel::Body { name }]);
    assert_eq!(picker.active, 1, "the active Body field follows its key");
    assert_eq!(picker.listed, Some(1), "the open pick list follows its field too");
}

#[test]
fn design_change_notice_only_requests_another_apply_after_stale_reply() {
    let mut h = box_and_pick_plugin();
    h.app.plugin_menu.listed[0].entries[0].input_schema["properties"]["edges"]["minItems"] = json!(0);
    open_form(&mut h.app, "inserts", "seats", &json!({"depth":2})).unwrap();
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1})).unwrap();
    let mut dialog = h.app.plugin_menu.dialogs.pop().unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    dialog.validation_gate = Some(Arc::new(std::sync::Mutex::new(wait)));
    let mut output = None;
    for _ in 0..3 {
        output = Some(form_frame(&h.ctx, |ui| {
            show_dialog(&mut h.app, ui.ctx(), &mut dialog);
        }));
    }
    let output = output.unwrap();
    let immediate = "The design changed. Review the inputs against the current design.";
    assert!(crate::test_support::text_shapes(&output).any(|t| t.galley.text() == immediate));
    assert!(crate::test_support::text_shapes(&output).any(|t| t.galley.text() == h.app.tr("ui.plugin_edit_checking")));
    assert!(!crate::test_support::text_shapes(&output).any(|t| t.galley.text() == h.app.tr("ui.plugin_edit_revalidated")));
    dialog.validation_gate = None;
    release.send(()).unwrap();
    h.app.plugin_menu.dialogs.push(dialog);
    checked_prefill(&h.app);
    let revision = h.app.session.design_revision();
    deliver_form_reply(&mut h.app, json!({"ok":true,"result":{"form_validation":{"revision":revision,"changed":true}}}));
    poll(&mut h.app);
    let mut dialog = h.app.plugin_menu.dialogs.pop().unwrap();
    let output = form_frame(&h.ctx, |ui| {
        show_dialog(&mut h.app, ui.ctx(), &mut dialog);
    });
    assert!(crate::test_support::text_shapes(&output).any(|t| t.galley.text() == h.app.tr("ui.plugin_edit_revalidated")));
    let id = dialog.id;
    h.app.plugin_menu.dialogs.push(dialog);
    run_dialog(&mut h.app, id).unwrap();
    let mut dialog = h.app.plugin_menu.dialogs.pop().unwrap();
    let output = form_frame(&h.ctx, |ui| {
        show_dialog(&mut h.app, ui.ctx(), &mut dialog);
    });
    assert!(!crate::test_support::text_shapes(&output).any(|t| t.galley.text() == h.app.tr("ui.plugin_edit_revalidated")));
}

#[test]
fn empty_forms_draw_checking_and_worker_failure_status() {
    let mut h = crate::scenario::Harness::new();
    let mut command = entry("empty", "edit", json!([]));
    command.input_schema = json!({"type":"object","properties":{}});
    h.app.plugin_menu.listed = vec![listed("empty", vec![command])];
    h.app.plugin_menu.loaded = true;
    open_form(&mut h.app, "empty", "empty", &json!({})).unwrap();
    let mut dialog = h.app.plugin_menu.dialogs.pop().unwrap();
    for key in ["ui.plugin_edit_checking", "ui.plugin_start_failed"] {
        let why = h.app.tr(key).to_owned();
        let output = form_frame(&h.ctx, |ui| {
            let _ = body(&h.app, ui, &mut dialog, Some(&why), &mut None, &mut None);
        });
        assert!(crate::test_support::text_shapes(&output).any(|t| t.galley.text() == why), "{key} must be visible without fields");
    }
}

#[test]
fn invalid_prefilled_values_and_conditional_choices_are_not_replaced_on_draw() {
    let mut h = crate::scenario::Harness::new();
    let mut command = entry("cabinet", "edit", json!([]));
    command.input_schema = cabinet_schema();
    h.app.plugin_menu.listed = vec![listed("cabinet", vec![command])];
    h.app.plugin_menu.loaded = true;
    open_form(&mut h.app, "cabinet", "cabinet", &json!({"storage":"drawers","drawer_count":-1,"size":"small","length":9007199254740993_u64}))
        .unwrap();
    h.app.plugin_menu.dialogs.last_mut().unwrap().preview = false;
    h.frames(3);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    let fields = dialog.fields.as_ref().unwrap();
    assert_eq!(fields.iter().find(|(f, _)| f.key == "size").unwrap().1, Input::Choice(Some(0)));
    assert_eq!(fields.iter().find(|(f, _)| f.key == "drawer_count").unwrap().1, Input::Typed("-1".into()));
    checked_prefill(&h.app);
    let errors = dialog.field_errors(&h.app);
    assert!(errors.contains_key("size") && errors.contains_key("drawer_count"), "{errors:?}");
    assert!(errors.contains_key("length"));
    assert_eq!(
        fields.iter().find(|(f, _)| f.key == "length").unwrap().1,
        Input::Raw("9007199254740993".into()),
        "unrepresentable values are retained as field errors"
    );
}

#[test]
fn edit_form_ui_start_opens_values_and_engine_execute_refuses_dialogs() {
    let mut h = box_and_pick_plugin();
    let args = json!({"plugin":"inserts","entry":"seats","params":{"depth":6,"edges":[],"body":[]}});
    let before = h.app.session.doc.clone();
    let refused = h.call("engine.execute", json!({"command":"plugin.edit","params":args}));
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["plugin_error"]["kind"], "interactive_required");
    assert!(h.app.plugin_menu.dialogs.is_empty());
    let opened = h.call("ui.start", json!({"command":"plugin.edit","params":args}));
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap().iter().find(|(f, _)| f.key == "depth").unwrap().1, Input::Number(6.0));
    assert_eq!(h.app.session.doc, before);
}

#[test]
fn edit_form_renders_prefilled_values_and_missing_pick_error() {
    let mut h = box_and_pick_plugin();
    h.app.home.open = false;
    h.ctx = crate::fonts::test_context();
    solidcraft_theme::apply(&h.ctx);
    let state = h.app.session.model.state();
    let body = state.bodies.first().unwrap();
    let point = solidcore_record::Vec3::new(10.0, 0.0, 0.0);
    let index = solidcraft_engine::kernel::blend_edges(&body.body, &[point]).unwrap()[0];
    let pick = json!({"type":"edge","body":body.name,"index":index,"point":[10,0,0]});
    let values = json!({"depth":7.5,"edges":[pick],"body":[{"type":"body","name":body.name}]});
    open_form(&mut h.app, "inserts", "seats", &values).unwrap();
    h.app.plugin_menu.dialogs.last_mut().unwrap().preview = false;
    let before = h.app.session.doc.clone();
    let mut textures = std::collections::HashMap::new();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-edit-form");
    std::fs::create_dir_all(&dir).unwrap();
    for stage in ["prefilled", "missing-pick"] {
        if stage == "missing-pick" {
            let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
            let fields = dialog.fields.as_mut().unwrap();
            fields.iter_mut().find(|(f, _)| f.key == "edges").unwrap().1 =
                Input::Selection(vec![Sel::Edge { body: "Deleted plate".into(), index, point }]);
            h.app.dialog = None;
        }
        checked_prefill(&h.app);
        let mut last = None;
        for _ in 0..5 {
            let mut output = h.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))),
                    time: Some(h.ctx.cumulative_frame_nr() as f64 / 15.0),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                },
            );
            ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            last = Some(output);
        }
        let output = last.unwrap();
        let dialog = h.app.plugin_menu.dialog().unwrap();
        if stage == "prefilled" {
            assert_eq!(dialog.ready(&h.app).unwrap()["depth"], 7.5);
            assert_eq!(picks(&h)[0].len(), 1);
            assert!(dialog.field_errors(&h.app).is_empty());
            assert!(crate::test_support::text_shapes(&output).any(|t| t.galley.text().contains("7.5")), "prefilled depth is drawn");
            let selected = solidcraft_ui_text::format("dialog.selected_count", &[("count", "1")]);
            assert!(
                crate::test_support::text_shapes(&output).any(|t| t.galley.text() == selected.strip_prefix("↖ ").unwrap_or(&selected)),
                "stored picks are drawn in the shared pick control"
            );
        } else {
            assert!(dialog.ready(&h.app).is_err());
            assert!(
                crate::test_support::text_shapes(&output).any(|t| t.galley.text() == h.app.tr("ui.plugin_edit_missing_pick")),
                "field error is drawn"
            );
            assert!(dialog.field_errors(&h.app).contains_key("edges"));
        }
        let image = ui_kit::test_raster(&h.ctx, output.shapes, output.pixels_per_point, h.ctx.viewport_rect(), &textures);
        assert_eq!(image.dimensions(), (1280, 720));
        image.save(dir.join(format!("{stage}.png"))).unwrap();
        assert_eq!(h.app.session.doc, before);
    }
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
    assert_eq!(fields[4].0.kind, Kind::Text { min_length: None, max_length: Some(8), directory: false });
    let angles: Vec<String> = vec!["14.5".into(), "20".into(), "25".into()];
    assert_eq!(fields[5].0.kind, Kind::Choice { values: angles.clone(), titles: angles }, "enum: each value shown as it is");
    assert_eq!(fields[5].1, Input::Choice(Some(1)), "the default");
    let no_default = form(&json!({"type":"object","properties":{"p":{"type":"string","enum":["a","b"]}}})).unwrap();
    assert_eq!(no_default[0].1, Input::Choice(None), "no default: nothing chosen yet");
    assert_eq!(form(&json!({"type":"object"})), Some(Vec::new()), "no properties: nothing to fill");
}

#[test]
fn shared_field_definitions_are_drawn_like_inline_fields() {
    let manifest = json!({"id":"sample","name":"Sample","version":"1","capabilities":["commands@0.1"],
        "$defs":{"angle":{"type":"string","title":"Angle","enum":["14.5","20","25"],"default":"20"}},
        "runtime":{"module":"plugin.wasm","commands":[],"entries":[{"name":"make","input_schema":{"type":"object",
            "properties":{"pressure_angle":{"$ref":"#/$defs/angle","default":"25"}}}}]}});
    let manifest = solidcraft_engine::plugin::parse_manifest(&serde_json::to_vec(&manifest).unwrap(), "sample").unwrap();
    let schema = &manifest.runtime.unwrap().entries[0].input_schema;
    let fields = form(schema).expect("a $ref field does not turn the dialog off");
    let angles: Vec<String> = vec!["14.5".into(), "20".into(), "25".into()];
    assert_eq!((fields[0].0.label.as_str(), &fields[0].0.kind), ("Angle", &Kind::Choice { values: angles.clone(), titles: angles }));
    assert_eq!(fields[0].1, Input::Choice(Some(2)), "the default beside the $ref wins");
}

#[test]
fn described_field_labels_and_values_show_the_same_tooltip() {
    for mut property in [
        json!({"type":"number","default":2}),
        json!({"type":"number"}),
        json!({"type":"integer","default":3}),
        json!({"type":"integer"}),
        json!({"type":"boolean"}),
        json!({"type":"string","enum":["one","two"]}),
        json!({"type":"string"}),
        json!({"type":"array","x-solidcraft-selection":{"version":1,"accept":["edge"]}}),
    ] {
        property["title"] = json!("Saw kerf");
        property["description"] = json!("Width removed by the saw blade");
        let mut h = crate::scenario::Harness::new();
        let mut command = entry("cut", "edit", json!([]));
        command.input_schema = json!({"type":"object","required":["kerf"],"properties":{"kerf":property}});
        h.app.plugin_menu.listed = vec![listed("saw", vec![command])];
        h.app.plugin_menu.loaded = true;
        h.app.home.open = false;
        open_entry(&mut h.app, 0, 0);
        h.app.plugin_menu.dialogs.last_mut().unwrap().preview = false;
        h.ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
        h.frames(4);
        // Selection fields have both an active native picker and an idle pick control.
        let selection = property["type"] == "array";
        for idle in 0..=usize::from(selection) {
            if idle == 1 {
                h.app.dialog = None;
                h.frames(3);
            }
            for label in [true, false] {
                h.frame_with(vec![egui::Event::PointerGone]);
                h.frames(3);
                let at = if label && property["type"] != "boolean" {
                    crate::scenario::rect_of("dialog:label:Saw kerf *").expect("field label").center()
                } else {
                    crate::scenario::handle_at("plugin_dialog:field:kerf").expect("field value")
                };
                h.frame_with(vec![egui::Event::PointerMoved(at)]);
                h.frames(4);
                let context = format!("{property}: label={label}, idle={idle}");
                let mut found = false;
                for shape in &h.shapes {
                    crate::test_support::each_shape(&shape.shape, &mut |shape| {
                        if let egui::Shape::Text(text) = shape {
                            found |= text.galley.text() == "Width removed by the saw blade";
                        }
                    });
                }
                assert!(found, "description is painted on hover: {context}");
            }
        }
    }
}

#[test]
fn fields_without_descriptions_keep_picking_help_and_clipped_label_tooltips() {
    let title = "A long plugin field label that must be clipped in the label column";
    let label = format!("{title} *");
    let mut h = crate::scenario::Harness::new();
    let mut command = entry("cut", "edit", json!([]));
    command.input_schema = json!({"type":"object","required":["kerf"],"properties":{
        "kerf":{"type":"array","title":title,"x-solidcraft-selection":{"version":1,"accept":["edge"]}}
    }});
    h.app.plugin_menu.listed = vec![listed("saw", vec![command])];
    h.app.plugin_menu.loaded = true;
    h.app.home.open = false;
    open_entry(&mut h.app, 0, 0);
    h.app.plugin_menu.dialogs.last_mut().unwrap().preview = false;
    h.ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
    h.frames(4);
    for idle in [false, true] {
        if idle {
            h.app.dialog = None;
            h.frames(3);
        }
        for hover_label in [false, true] {
            h.frame_with(vec![egui::Event::PointerGone]);
            h.frames(3);
            if hover_label {
                let mut clipped = false;
                for shape in &h.shapes {
                    crate::test_support::each_shape(&shape.shape, &mut |shape| {
                        if let egui::Shape::Text(text) = shape {
                            clipped |= text.galley.text() == label && text.galley.elided;
                        }
                    });
                }
                assert!(clipped, "the label must actually be clipped: idle={idle}");
            }
            let at = if hover_label {
                crate::scenario::rect_of(&format!("dialog:label:{label}")).expect("field label").center()
            } else {
                crate::scenario::handle_at("plugin_dialog:field:kerf").expect("selection value")
            };
            h.frame_with(vec![egui::Event::PointerMoved(at)]);
            h.frames(4);
            let expected = if hover_label { label.as_str() } else { "click edges or faces (click to pick into this input)" };
            let mut found = false;
            for shape in &h.shapes {
                crate::test_support::each_shape(&shape.shape, &mut |shape| {
                    if let egui::Shape::Text(text) = shape {
                        found |= text.galley.text() == expected && !text.galley.elided;
                    }
                });
            }
            assert!(found, "default tooltip is painted: label={hover_label}, idle={idle}, expected={expected}");
        }
    }
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
fn swatch_annotations_preserve_partial_choices_and_const_values() {
    let schema = json!({"type":"object","properties":{"finish":{"type":"string","oneOf":[
        {"const":"oak","title":"Oak","x-solidcraft-swatch":"oak.png"},
        {"const":"walnut","title":"Walnut","x-solidcraft-swatch":7},
        {"const":"plain","title":"Plain"}
    ]}}});
    let mut fields = form(&schema).expect("bad artwork does not disable the dialog");
    assert_eq!(fields[0].0.swatches, [Some("oak.png".into()), None, None]);
    fields[0].1 = Input::Choice(Some(1));
    assert_eq!(params(&fields), Ok(json!({"finish":"walnut"})));
}

/// Two full dialog captures, with synthetic artwork generated here, plus pointer selection.
#[test]
fn swatch_dialog_dropdown_and_filtered_grid_render_and_send_const_values() {
    let mut h = crate::scenario::Harness::new();
    let dir = h.dir.join("swatches");
    std::fs::create_dir_all(&dir).unwrap();
    for (path, base) in [("oak.png", [137, 61, 23]), ("walnut.jpg", [81, 41, 27])] {
        let pixels = image::RgbImage::from_fn(128, 128, |x, y| {
            let stripe = ((x / 8 + y / 16) % 2) as u8 * 16;
            image::Rgb(base.map(|c| c + stripe))
        });
        pixels.save(dir.join(path)).unwrap();
    }
    let schema = |complete| {
        json!({"type":"object","required":["finish"],"properties":{
            "show_unavailable":{"type":"boolean","default":false},
            "finish":{"type":"string","title":"Finish","oneOf":[
            {"const":"unavailable","title":"Unavailable finish","x-solidcraft-swatch":"walnut.jpg",
             "x-solidcraft-when":{"field":"show_unavailable","eq":true}},
            {"const":"oak","title":"Natural oak","x-solidcraft-swatch":"oak.png"},
            {"const":"walnut","title":"Walnut","x-solidcraft-swatch":"walnut.jpg"},
            if complete { json!({"const":"white","title":"White oak","x-solidcraft-swatch":"oak.png"}) } else { json!({"const":"plain","title":"Plain"}) }
        ]}}})
    };
    let ctx = crate::fonts::test_context();
    let mut textures = std::collections::HashMap::new();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    let output_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-swatches");
    std::fs::create_dir_all(&output_dir).unwrap();
    let mut tick = 0;
    let mut frame = |app: &mut SolveApp, events: Vec<egui::Event>, capture: Option<&str>| {
        tick += 1;
        solidcraft_ui_registry::clear_handles();
        let mut output =
            ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                ui_kit::scope(ui, |_ui| show(app, &ctx));
            });
        ui_kit::test_textures_update(&mut textures, &output.textures_delta);
        output.textures_delta.clear();
        if let Some(name) = capture {
            let image = ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
            assert_eq!(image.dimensions(), (1280, 720));
            image.save(output_dir.join(name)).unwrap();
        }
    };
    let pointer = |pos, pressed| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]
    };
    for complete in [false, true] {
        let mut command = entry("finish", "read", json!([]));
        command.input_schema = schema(complete);
        let mut plugin = listed(if complete { "grid" } else { "dropdown" }, vec![command]);
        plugin.swatches = Arc::new(std::sync::OnceLock::from(crate::plugin_images::read_swatches(&dir, &plugin.entries)));
        h.app.plugin_menu.listed = vec![plugin];
        h.app.plugin_menu.loaded = true;
        h.app.plugin_menu.dialogs.clear();
        open_entry(&mut h.app, 0, 0);
        for _ in 0..6 {
            frame(&mut h.app, vec![], None);
        }
        let trigger = crate::scenario::handle_at("plugin_dialog:field:finish").unwrap();
        frame(&mut h.app, pointer(trigger, true), None);
        frame(&mut h.app, pointer(trigger, false), None);
        for _ in 0..3 {
            frame(&mut h.app, vec![], None);
        }
        if complete {
            let search = crate::scenario::handle_at("swatch:plugin_field_finish:search").unwrap();
            frame(&mut h.app, pointer(search, true), None);
            frame(&mut h.app, pointer(search, false), None);
            frame(&mut h.app, vec![egui::Event::Text("OaK".into())], None);
            frame(&mut h.app, vec![], Some("grid-filtered.png"));
            assert!(crate::scenario::handle_at("swatch:plugin_field_finish:option:1").is_none(), "Walnut is filtered out");
            assert!(crate::scenario::handle_at("swatch:plugin_field_finish:option:0").is_some());
        } else {
            frame(&mut h.app, vec![], Some("dropdown.png"));
            assert!(crate::scenario::handle_at("swatch:plugin_field_finish:search").is_none());
            assert!(crate::scenario::rect_of("swatch:plugin_field_finish:tile:0").is_some());
            assert!(crate::scenario::rect_of("swatch:plugin_field_finish:tile:1").is_some());
            assert!(crate::scenario::rect_of("swatch:plugin_field_finish:tile:2").is_none());
        }
        let select = crate::scenario::handle_at("swatch:plugin_field_finish:option:2").unwrap();
        frame(&mut h.app, pointer(select, true), None);
        frame(&mut h.app, pointer(select, false), None);
        let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
        let (finish, selected) = fields.iter().find(|(field, _)| field.key == "finish").unwrap();
        assert_eq!(finish.available, [1, 2, 3], "conditional choices retain their schema indices");
        assert_eq!(*selected, Input::Choice(Some(3)), "picker index maps back past the unavailable option");
        assert_eq!(params(fields), Ok(json!({"finish": if complete { "white" } else { "plain" }, "show_unavailable":false})));
        frame(&mut h.app, vec![], None);
        assert!(crate::scenario::handle_at("swatch:plugin_field_finish:option:0").is_none(), "selection closes the picker");

        // A stored, now unavailable schema choice must survive drawing either swatch layout.
        h.app.plugin_menu.dialogs.clear();
        open_form(&mut h.app, if complete { "grid" } else { "dropdown" }, "finish", &json!({"finish":"unavailable"})).unwrap();
        for _ in 0..6 {
            frame(&mut h.app, vec![], None);
        }
        let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
        let (finish, selected) = fields.iter().find(|(field, _)| field.key == "finish").unwrap();
        assert!(finish.prefilled);
        assert_eq!(finish.available, [1, 2, 3]);
        assert_eq!(*selected, Input::Choice(Some(0)), "drawing preserves the unavailable stored choice");
        assert_eq!(params(fields), Err(Problem::Invalid("Finish".into())), "the stored choice needs correction");

        let trigger = crate::scenario::handle_at("plugin_dialog:field:finish").unwrap();
        frame(&mut h.app, pointer(trigger, true), None);
        frame(&mut h.app, pointer(trigger, false), None);
        for _ in 0..3 {
            frame(&mut h.app, vec![], None);
        }
        let select = crate::scenario::handle_at("swatch:plugin_field_finish:option:2").unwrap();
        frame(&mut h.app, pointer(select, true), None);
        frame(&mut h.app, pointer(select, false), None);
        let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
        assert_eq!(
            fields.iter().find(|(field, _)| field.key == "finish").unwrap().1,
            Input::Choice(Some(3)),
            "an explicit swatch selection corrects the stored choice using its schema index"
        );
        assert_eq!(params(fields), Ok(json!({"finish": if complete { "white" } else { "plain" }, "show_unavailable":false})));
    }
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

fn cabinet_schema() -> Value {
    json!({"type":"object","required":["storage","shelf_count","drawer_count"],"properties":{
        "storage":{"type":"string","title":"Inside","default":"shelves","oneOf":[{"const":"shelves","title":"Shelves"},{"const":"drawers","title":"Drawers"}]},
        "shelf_count":{"type":"integer","title":"Shelves","default":3,"minimum":1,"x-solidcraft-visible-when":{"field":"storage","eq":"shelves"}},
        "drawer_count":{"type":"integer","title":"Drawers","default":4,"minimum":1,"x-solidcraft-visible-when":{"field":"storage","eq":"drawers"}},
        "has_back":{"type":"boolean","title":"Back panel","default":true},
        "back_thickness":{"type":"number","title":"Back thickness","default":6.0,"x-solidcraft-enabled-when":{"field":"has_back","eq":true}},
        "size":{"type":"string","title":"Size","default":"small","oneOf":[
            {"const":"small","x-solidcraft-when":{"field":"storage","eq":"shelves"}},
            {"const":"medium","x-solidcraft-when":{"field":"storage","eq":"drawers"}},
            {"const":"large","x-solidcraft-when":{"field":"storage","eq":"drawers"}}],
            "x-solidcraft-defaults":[{"when":{"field":"storage","eq":"drawers"},"value":"large"}]},
        "length":{"type":"number","title":"Length","default":20.0,"x-solidcraft-defaults":[
            {"when":{"field":"storage","eq":"drawers"},"value":30.0},
            {"when":{"field":"storage","eq":"shelves"},"value":20.0}]}
    }})
}

fn set_input(fields: &mut [(Field, Input)], key: &str, input: Input) {
    let (field, value) = fields.iter_mut().find(|(f, _)| f.key == key).unwrap();
    *value = input;
    field.edited = true;
    field.folder_chosen |= field.grant_read;
    refresh_conditions(fields);
}

#[test]
fn conditional_dialog_fields_hide_disable_reset_and_follow_defaults_until_edited() {
    let mut fields = form(&cabinet_schema()).expect("conditional form");
    assert_eq!(params(&fields).unwrap().get("drawer_count"), None);
    set_input(&mut fields, "storage", Input::Choice(Some(1)));
    let sent = params(&fields).unwrap();
    assert_eq!(sent.get("shelf_count"), None);
    assert_eq!(sent["drawer_count"], 4);
    assert_eq!(sent["size"], "large", "matching default before first valid choice");
    assert_eq!(sent["length"], 30.0);
    set_input(&mut fields, "has_back", Input::Boolean(false));
    assert!(!fields.iter().find(|(f, _)| f.key == "back_thickness").unwrap().0.enabled);
    assert_eq!(params(&fields).unwrap()["back_thickness"], 6.0, "disabled values are sent");
    set_input(&mut fields, "length", Input::Number(42.0));
    set_input(&mut fields, "storage", Input::Choice(Some(0)));
    assert_eq!(params(&fields).unwrap()["length"], 42.0, "custom value survives");
    assert_eq!(params(&fields).unwrap()["size"], "small", "an unavailable custom choice resets");
}

#[test]
fn conditional_swatches_keep_defaults_visibility_and_enabled_state() {
    let mut schema = cabinet_schema();
    let size = &mut schema["properties"]["size"];
    for (option, path) in size["oneOf"].as_array_mut().unwrap().iter_mut().zip(["small.png", "medium.jpg", "large.png"]) {
        option["x-solidcraft-swatch"] = json!(path);
    }
    size["x-solidcraft-visible-when"] = json!({"field":"has_back","eq":true});
    size["x-solidcraft-enabled-when"] = json!({"field":"storage","eq":"shelves"});
    let mut fields = form(&schema).expect("conditions and swatches are supported together");
    let size = |fields: &[(Field, Input)]| fields.iter().find(|(f, _)| f.key == "size").unwrap().clone();
    assert_eq!(size(&fields).0.swatches, [Some("small.png".into()), Some("medium.jpg".into()), Some("large.png".into())]);
    set_input(&mut fields, "storage", Input::Choice(Some(1)));
    assert_eq!(size(&fields).0.available, [1, 2]);
    assert!(!size(&fields).0.enabled);
    assert_eq!(params(&fields).unwrap()["size"], "large", "a disabled swatch field still sends its conditional default");
    set_input(&mut fields, "has_back", Input::Boolean(false));
    assert!(!size(&fields).0.visible);
    assert!(params(&fields).unwrap().get("size").is_none(), "hidden swatch fields are omitted");
    set_input(&mut fields, "has_back", Input::Boolean(true));
    set_input(&mut fields, "size", Input::Choice(Some(1)));
    set_input(&mut fields, "storage", Input::Choice(Some(0)));
    assert!(size(&fields).0.enabled);
    assert_eq!(params(&fields).unwrap()["size"], "small", "an unavailable swatch choice resets to the matching default");
}

#[test]
fn conditional_choices_reset_in_schema_order_and_recover_after_no_available_options() {
    let schema = json!({"type":"object","required":["size"],"properties":{
        "mode":{"type":"string","enum":["a","b","none"],"default":"a"},
        "size":{"type":"string","default":"one","oneOf":[
            {"const":"one","x-solidcraft-when":{"field":"mode","eq":"a"}},
            {"const":"two","x-solidcraft-when":{"field":"mode","eq":"b"}},
            {"const":"three","x-solidcraft-when":{"field":"mode","eq":"b"}}]}
    }});
    let mut fields = form(&schema).unwrap();
    set_input(&mut fields, "mode", Input::Choice(Some(1)));
    assert_eq!(params(&fields).unwrap()["size"], "two");
    set_input(&mut fields, "mode", Input::Choice(Some(2)));
    assert_eq!(params(&fields), Err(Problem::Missing("size".into())));
    set_input(&mut fields, "mode", Input::Choice(Some(1)));
    assert_eq!(params(&fields).unwrap()["size"], "two");
}

#[test]
fn conditional_defaults_cascade_and_hidden_empty_fields_do_not_gate_run() {
    let schema = json!({"type":"object","required":["empty"],"properties":{
        "z_mode":{"type":"boolean","default":false},
        "b_value":{"type":"integer","default":1,"x-solidcraft-defaults":[{"when":{"field":"z_mode","eq":true},"value":2}]},
        "a_value":{"type":"integer","default":10,"x-solidcraft-defaults":[{"when":{"field":"b_value","eq":2},"value":20}]},
        "empty":{"type":"number","x-solidcraft-visible-when":{"field":"z_mode","eq":false}}
    }});
    let mut fields = form(&schema).unwrap();
    assert_eq!(params(&fields), Err(Problem::Missing("empty".into())));
    set_input(&mut fields, "z_mode", Input::Boolean(true));
    assert_eq!(params(&fields), Ok(json!({"z_mode":true,"b_value":2,"a_value":20})));
}

#[test]
fn conditional_dialog_and_composed_input_share_hidden_controllers() {
    let controller = json!({"gate":{"type":"boolean","default":false},"mode":{"type":"boolean","default":true,"x-solidcraft-visible-when":{"field":"gate","eq":true}}});
    let dependent = json!({"mode":{"type":"boolean"},"secret":{"type":"integer","default":7,"x-solidcraft-visible-when":{"field":"mode","eq":true}}});
    let composed =
        json!({"type":"object","additionalProperties":true,"properties":controller,"oneOf":[{"required":["secret"],"properties":dependent}]});
    // Composition remains a CLI/agent form. Its equivalent dialog must send the same values.
    let mut flat = json!({"type":"object","required":["secret"],"properties":controller});
    flat["properties"]["secret"] = dependent["secret"].clone();
    let mut fields = form(&flat).unwrap();
    for gate in [false, true, false] {
        set_input(&mut fields, "gate", Input::Boolean(gate));
        let expected = if gate { json!({"gate":true,"mode":true,"secret":7}) } else { json!({"gate":false}) };
        assert_eq!(params(&fields), Ok(expected));
        let secret = if gate { json!(7) } else { json!("hidden") };
        assert_eq!(solidcraft_engine::plugin::check_input(&composed, &json!({"gate":gate,"mode":true,"secret":secret})), Ok(()));
        for key in ["mode", "secret"] {
            assert_eq!(fields.iter().find(|(f, _)| f.key == key).unwrap().0.visible, gate);
        }
    }
}

#[test]
fn invalid_conditional_dialog_schemas_leave_run_off() {
    for condition in [json!({"field":"missing","eq":true}), json!({"field":"a","eq":1})] {
        assert!(form(&json!({"type":"object","properties":{"a":{"type":"number","x-solidcraft-visible-when":condition}}})).is_none());
    }
}

fn hook_dialog_harness() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    let mut command = entry("lookup", "read", json!([]));
    command.dialog_hook = true;
    command.input_schema = json!({"type":"object","required":["width"],"properties":{
        "width":{"type":"number","title":"Width","default":1,"minimum":1},
        "size":{"type":"string","title":"Size","default":"large","oneOf":[{"const":"small"},{"const":"large"}]}
    }});
    h.app.plugin_menu.listed = vec![listed("lookup", vec![command])];
    h.app.plugin_menu.loaded = true;
    h.app.home.open = false;
    open_entry(&mut h.app, 0, 0);
    h
}

fn computed_hook_reply(dialog: &EntryDialog) -> solidcraft_engine::plugin::dialog::Update {
    let hook = dialog.hook.as_ref().unwrap();
    solidcraft_engine::plugin::dialog::apply(
        &hook.schema,
        &json!({"width":1,"size":"large"}),
        &Default::default(),
        Some(r#"{"width":{"default":24},"size":{"choices":["large"]}}"#),
        false,
    )
    .unwrap()
}

#[test]
fn exporter_dialog_keeps_hook_values_and_waits_for_its_session_before_showing_completion() {
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.writes = Some(vec!["smp".into()]);
    let update = computed_hook_reply(dialog);
    assert!(apply_hook_reply(dialog, 0, update));
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap()["width"], 24.0);
    deliver_form_reply(&mut h.app, json!({"ok":true,"result":{"path":"bracket.smp","warnings":["export note"]}}));
    crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
    poll(&mut h.app);
    assert!(h.app.plugin_menu.dialog().unwrap().running.as_ref().unwrap().received.is_some());
    crate::documents::switch(&mut h.app, 0);
    poll(&mut h.app);
    assert!(h.app.plugin_menu.dialogs.is_empty());
    let (message, _, error) = h.app.status.as_ref().unwrap();
    assert!(!*error);
    assert!(message.contains("bracket.smp") && message.contains("export note"));
}

#[test]
fn export_save_callback_queues_for_the_dialogs_original_session() {
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialog().unwrap();
    let (id, session) = (dialog.id, dialog.session);
    crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
    queue(&mut h.app, id, json!({"command":"file.export","params":{"path":"bracket.smp"}}), None);
    let request = h.app.control_held.pop_back().unwrap();
    assert_eq!(h.app.plugin_menu.pending.last(), Some(&(request.serial, session)));
    assert!(check_dispatch(&mut h.app, &request).is_err(), "a file picker cannot redirect an export to another design");
    h.app.plugin_menu.dialogs.clear();
    queue(&mut h.app, id, json!({"command":"file.export","params":{}}), None);
    assert!(h.app.control_held.is_empty(), "a dismissed dialog cannot queue an export");
}

#[test]
fn dialog_hook_drops_stale_generations_and_never_overwrites_user_edits() {
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let reply = computed_hook_reply(dialog);
    dialog.hook.as_mut().unwrap().generation = 2;
    assert!(!apply_hook_reply(dialog, 1, reply.clone()));
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["width"], 1.0);
    set_input(dialog.fields.as_mut().unwrap(), "width", Input::Number(42.0));
    assert!(apply_hook_reply(dialog, 2, reply));
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["width"], 42.0);
}

#[test]
fn dialog_hook_prefilled_values_stay_protected_and_hidden_values_are_validated() {
    let mut h = hook_dialog_harness();
    h.app.plugin_menu.dialogs.clear();
    open_form(&mut h.app, "lookup", "lookup", &json!({"width":42,"size":"small"})).unwrap();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let fields = dialog.fields.as_ref().unwrap();
    let protected = fields.iter().filter(|(field, _)| field.edited).map(|(field, _)| field.key.clone()).collect();
    let update = solidcraft_engine::plugin::dialog::apply(
        &dialog.hook.as_ref().unwrap().schema,
        &hook_values(fields),
        &protected,
        Some(r#"{"width":{"visible":false,"default":24,"hint":"Computed width"},"size":{"enabled":false,"choices":["large"]}}"#),
        false,
    )
    .unwrap();
    assert!(apply_hook_reply(dialog, 0, update));
    let fields = dialog.fields.as_ref().unwrap();
    let (width, value) = fields.iter().find(|(field, _)| field.key == "width").unwrap();
    assert!(!width.visible && width.prefilled && width.edited);
    assert_eq!(*value, Input::Number(42.0), "a hook default must not replace a stored value");
    assert_eq!(width.tip.as_deref(), Some("Computed width"));
    let (size, value) = fields.iter().find(|(field, _)| field.key == "size").unwrap();
    assert!(!size.enabled);
    assert_eq!(size.available, [1]);
    assert_eq!(*value, Input::Choice(Some(0)), "an unavailable stored choice remains visible for correction");
    checked_prefill(&h.app);
    let errors = h.app.plugin_menu.dialog().unwrap().field_errors(&h.app);
    assert!(errors.contains_key("size"), "a narrowed stored choice is invalid");
    assert!(!errors.contains_key("width"), "a hook-hidden required value is still sent and validated");

    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    set_input(dialog.fields.as_mut().unwrap(), "size", Input::Choice(Some(1)));
    checked_prefill(&h.app);
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap(), json!({"width":42.0,"size":"large"}));
}

#[test]
fn dialog_hook_defaults_take_precedence_after_computed_controllers_change() {
    let schema = json!({"type":"object","properties":{
        "mode":{"type":"boolean","default":false},
        "width":{"type":"number","default":3,"x-solidcraft-defaults":[{"when":{"field":"mode","eq":true},"value":6}]}
    }});
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields = form(&schema);
    dialog.hook.as_mut().unwrap().schema = schema.clone();
    let update = solidcraft_engine::plugin::dialog::apply(
        &schema,
        &json!({}),
        &Default::default(),
        Some(r#"{"mode":{"value":true},"width":{"default":24}}"#),
        false,
    )
    .unwrap();
    apply_hook_reply(dialog, 0, update);
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap(), json!({"mode":true,"width":24.0}));
    refresh_conditions(dialog.fields.as_mut().unwrap());
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["width"], 24.0);
}

#[test]
fn dialog_hook_narrowing_preserves_an_edited_choice_until_the_user_selects_an_available_one() {
    for artwork in ["none", "partial", "complete"] {
        let mut h = hook_dialog_harness();
        let plugin = &mut h.app.plugin_menu.listed[0];
        let options = &mut plugin.entries[0].input_schema["properties"]["size"]["oneOf"];
        *options = json!([{"const":"small"},{"const":"medium"},{"const":"large"}]);
        if artwork != "none" {
            image::RgbaImage::from_pixel(2, 2, image::Rgba([100, 150, 200, 255])).save(h.dir.join("size.png")).unwrap();
            options[2]["x-solidcraft-swatch"] = json!("size.png");
            if artwork == "complete" {
                options[1]["x-solidcraft-swatch"] = json!("size.png");
            }
            plugin.swatches = Arc::new(std::sync::OnceLock::from(crate::plugin_images::read_swatches(&h.dir, &plugin.entries)));
            assert_eq!(plugin.swatches.get().unwrap().len(), 1, "the swatch path must be exercised");
        }
        h.app.plugin_menu.dialogs.clear();
        open_entry(&mut h.app, 0, 0);
        let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
        set_input(dialog.fields.as_mut().unwrap(), "size", Input::Choice(Some(0)));
        let update = solidcraft_engine::plugin::dialog::apply(
            &dialog.hook.as_ref().unwrap().schema,
            &json!({"width":1,"size":"small"}),
            &std::collections::BTreeSet::from(["size".into()]),
            Some(r#"{"size":{"choices":["medium","large"]}}"#),
            false,
        )
        .unwrap();
        apply_hook_reply(dialog, 0, update);
        h.frames(3);
        let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
        let (field, input) = fields.iter().find(|(f, _)| f.key == "size").unwrap();
        assert_eq!(field.available, [1, 2]);
        assert_eq!(scalar_value(field, input), Some(json!("small")), "painting must not replace an edited choice: {artwork}");
        assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_err());
        h.step(&json!({"click":{"handle":"plugin_dialog:field:size"}})).unwrap();
        let option = if artwork == "none" { "dialog:Size:large" } else { "swatch:plugin_field_size:option:1" };
        h.step(&json!({"click":{"handle":option}})).unwrap();
        let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
        assert_eq!(fields.iter().find(|(f, _)| f.key == "size").unwrap().1, Input::Choice(Some(2)), "picker indices map to schema indices");
        assert_eq!(params(fields).unwrap()["size"], "large");
    }
}

#[test]
fn dialog_hook_selection_defaults_preserve_viewport_edits() {
    let schema = json!({"type":"object","properties":{"bodies":{"type":"array","items":{"type":"string"},"maxItems":1}}});
    let mut h = hook_dialog_harness();
    let doc = h.app.session.doc.clone();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields = form(&schema);
    dialog.hook.as_mut().unwrap().schema = schema.clone();
    let field = &dialog.fields.as_ref().unwrap()[0].0;
    assert_eq!(default_input(field, &json!(["computed"])), Some(Input::Selection(vec![Sel::Body { name: "computed".into() }])));
    let mut picker = crate::dialogs::Dialog::new(crate::dialogs::Kind::PluginPick { dialog: dialog.id }, dialog.selection_inputs());
    picker.inputs[0].items = vec![Sel::Body { name: "chosen".into() }];
    take_picks(dialog, &picker, &doc);
    assert!(dialog.fields.as_ref().unwrap()[0].0.edited, "viewport picks are user edits");
    let update =
        solidcraft_engine::plugin::dialog::apply(&schema, &json!({}), &Default::default(), Some(r#"{"bodies":{"default":["computed"]}}"#), false)
            .unwrap();
    apply_hook_reply(dialog, 0, update);
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["bodies"], json!(["chosen"]));
}

#[test]
fn dialog_hook_discards_selection_baselines_when_the_document_changes() {
    let schema = json!({"type":"object","properties":{
        "bodies":{"type":"array","items":{"type":"string"},"maxItems":1},
        "width":{"type":"number","default":1}
    }});
    for changed_document in [false, true] {
        let mut h = hook_dialog_harness();
        let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
        dialog.fields = form(&schema);
        dialog.hook.as_mut().unwrap().schema = schema.clone();
        let (_, input) = dialog.fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "bodies").unwrap();
        *input = Input::Selection(vec![Sel::Body { name: "prefilled-a".into() }]);
        let update = solidcraft_engine::plugin::dialog::apply(
            &schema,
            &json!({"bodies":["prefilled-a"],"width":1}),
            &Default::default(),
            Some(r#"{"bodies":{"value":["computed-b"]},"width":{"value":24}}"#),
            false,
        )
        .unwrap();
        assert!(apply_hook_reply(dialog, 0, update));
        dialog.picked_on = Some(h.app.session.doc.clone());
        let (send, receive) = std::sync::mpsc::channel();
        dialog.hook.as_mut().unwrap().pending = Some((0, receive));
        if changed_document {
            Arc::make_mut(&mut h.app.session.doc).name = "Changed design".into();
            sync_picks(&mut h.app);
        } else {
            set_input(h.app.plugin_menu.dialogs.last_mut().unwrap().fields.as_mut().unwrap(), "width", Input::Number(12.0));
        }
        poll_hooks(&mut h.app, &h.ctx);
        let dialog = h.app.plugin_menu.dialog().unwrap();
        let values = hook_values(dialog.fields.as_ref().unwrap());
        let expected = (!changed_document).then(|| json!(["prefilled-a"]));
        assert_eq!(values.get("bodies"), expected.as_ref(), "no old selection may be sent to the next hook");
        assert_eq!(values["width"], if changed_document { 1.0 } else { 12.0 }, "ordinary field baselines still restore");
        let baselines = dialog.fields.as_ref().unwrap().iter().filter(|(f, _)| f.key == "bodies").all(|(f, _)| f.hook_baseline.is_none());
        assert_eq!(baselines, changed_document, "only a document change drops the hook's selection");
        assert!(dialog.hook.as_ref().unwrap().generation > 0);
        drop(send);
    }
}

#[test]
fn dialog_hook_keeps_its_last_reply_on_screen_while_the_next_one_is_pending() {
    let schema = json!({"type":"object","properties":{
        "reveal":{"type":"number","default":2},
        "plinth":{"type":"boolean","default":false},
        "plinth_type":{"type":"string","default":"legs","oneOf":[{"const":"legs"},{"const":"box"}],
            "x-solidcraft-visible-when":{"field":"plinth","eq":true}}
    }});
    let reply = || {
        solidcraft_engine::plugin::dialog::apply(
            &schema,
            &json!({"reveal":2,"plinth":false,"plinth_type":"legs"}),
            &Default::default(),
            Some(r#"{"plinth":{"default":true}}"#),
            false,
        )
        .unwrap()
    };
    let shown = |dialog: &EntryDialog| {
        let fields = dialog.fields.as_ref().unwrap();
        (fields.iter().map(|(_, i)| i.clone()).collect::<Vec<_>>(), fields.iter().map(|(f, _)| f.visible).collect::<Vec<_>>())
    };
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields = form(&schema);
    dialog.hook.as_mut().unwrap().schema = schema.clone();
    assert!(apply_hook_reply(dialog, 0, reply()));
    set_input(dialog.fields.as_mut().unwrap(), "reveal", Input::Number(3.0));
    let before = shown(dialog);
    poll_hooks(&mut h.app, &h.ctx);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let generation = dialog.hook.as_ref().unwrap().generation;
    assert!(generation > 0, "the change starts a new generation");
    assert_eq!(shown(dialog), before, "nothing moves while the next reply is pending");
    let fields = dialog.fields.as_ref().unwrap();
    assert_eq!(params(fields).unwrap(), json!({"reveal":3.0,"plinth":true,"plinth_type":"legs"}));
    assert_eq!(hook_values(fields)["plinth"], false, "the hook still gets the value from before its default");
    assert!(apply_hook_reply(dialog, generation, reply()));
    assert_eq!(shown(dialog), before, "an equal reply changes nothing");
}

#[test]
fn dialog_hook_request_keeps_a_selection_its_pre_hook_controller_shows() {
    let schema = json!({"type":"object","properties":{
        "plinth":{"type":"boolean","default":true},
        "bodies":{"type":"array","items":{"type":"string"},"maxItems":1,"x-solidcraft-visible-when":{"field":"plinth","eq":true}}
    }});
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields = form(&schema);
    dialog.hook.as_mut().unwrap().schema = schema.clone();
    let (_, input) = dialog.fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "bodies").unwrap();
    *input = Input::Selection(vec![Sel::Body { name: "a".into() }]);
    let update = solidcraft_engine::plugin::dialog::apply(
        &schema,
        &json!({"plinth":true,"bodies":["a"]}),
        &Default::default(),
        Some(r#"{"plinth":{"value":false}}"#),
        false,
    )
    .unwrap();
    assert!(apply_hook_reply(dialog, 0, update));
    let fields = dialog.fields.as_ref().unwrap();
    assert!(!fields.iter().find(|(f, _)| f.key == "bodies").unwrap().0.visible, "the hook's value hides the selection on screen");
    let values = hook_values(fields);
    assert_eq!(values["plinth"], true);
    assert_eq!(values.get("bodies"), Some(&json!(["a"])), "the next hook still gets the selection its sent controller shows");
}

#[test]
fn dialog_hook_restored_field_takes_the_conditional_default_its_controller_now_gives() {
    let schema = json!({"type":"object","properties":{
        "mode":{"type":"boolean","default":false},
        "depth":{"type":"number","default":1,"x-solidcraft-defaults":[{"when":{"field":"mode","eq":true},"value":5}]}
    }});
    let reply =
        |values: Value, patch: &str| solidcraft_engine::plugin::dialog::apply(&schema, &values, &Default::default(), Some(patch), false).unwrap();
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields = form(&schema);
    dialog.hook.as_mut().unwrap().schema = schema.clone();
    assert!(apply_hook_reply(dialog, 0, reply(json!({"mode":false,"depth":1}), r#"{"depth":{"default":24}}"#)));
    set_input(dialog.fields.as_mut().unwrap(), "mode", Input::Boolean(true));
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["depth"], 24.0, "the hook value stays while its reply is pending");
    assert!(apply_hook_reply(dialog, 0, reply(json!({"mode":true,"depth":1}), "{}")));
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["depth"], 5.0, "the reply dropped depth, so mode's default applies");
}

#[test]
fn dialog_hook_pending_rejects_invalid_current_inputs_instead_of_cached_parameters() {
    let mut h = hook_dialog_harness();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    set_input(dialog.fields.as_mut().unwrap(), "width", Input::Number(12.0));
    poll_hooks(&mut h.app, &h.ctx);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let generation = dialog.hook.as_ref().unwrap().generation;
    dialog.hook.as_mut().unwrap().pending = Some((generation, receive));
    let id = dialog.id;
    let queued = h.app.control_held.len();
    for input in [Input::Typed(String::new()), Input::Typed("invalid".into()), Input::Number(0.0)] {
        set_input(h.app.plugin_menu.dialogs.last_mut().unwrap().fields.as_mut().unwrap(), "width", input);
        assert!(run_dialog(&mut h.app, id).is_err(), "invalid current fields must not submit the cached width");
        assert_eq!(h.app.control_held.len(), queued);
        assert!(h.app.plugin_menu.dialog().unwrap().running.is_none());
    }
    set_input(h.app.plugin_menu.dialogs.last_mut().unwrap().fields.as_mut().unwrap(), "width", Input::Number(18.0));
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap()["width"], 18.0);
    run_dialog(&mut h.app, id).unwrap();
    assert_eq!(h.app.control_held.len(), queued + 1);
    assert_eq!(h.app.control_held.back().unwrap().params["params"]["params"]["width"], 18.0);
    assert!(h.app.plugin_menu.dialog().unwrap().running.is_some());
    drop(send);
}

#[test]
fn dialog_hook_pending_accepts_widget_edits_and_drops_the_old_worker_reply() {
    let mut h = hook_dialog_harness();
    let (send, receive) = std::sync::mpsc::channel();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let reply = computed_hook_reply(dialog);
    dialog.hook.as_mut().unwrap().pending = Some((0, receive));
    h.frames(4);
    h.step(&json!({"click":{"handle":"plugin_dialog:field:size"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Size:small"}})).unwrap();
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["size"], "small");
    assert!(dialog.ready(&h.app).is_ok());
    assert!(dialog.hook.as_ref().unwrap().generation > 0);
    send.send(Ok(reply)).unwrap();
    h.frames(1);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["width"], 1.0, "the stale default was dropped");
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["size"], "small");
}

#[test]
fn dialog_hook_wait_keeps_fields_and_run_interactive_and_renders_computed_default() {
    let mut h = hook_dialog_harness();
    let (send, receive) = std::sync::mpsc::channel();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let generation = dialog.hook.as_ref().unwrap().generation;
    let reply = computed_hook_reply(dialog);
    dialog.hook.as_mut().unwrap().pending = Some((generation, receive));
    h.ctx = crate::fonts::test_context();
    solidcraft_theme::apply(&h.ctx);
    let mut textures = std::collections::HashMap::new();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-dialog-hook");
    std::fs::create_dir_all(&dir).unwrap();
    for stage in ["before", "after"] {
        let mut output = None;
        for _ in 0..5 {
            let mut frame = h.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))),
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                },
            );
            ui_kit::test_textures_update(&mut textures, &frame.textures_delta);
            frame.textures_delta.clear();
            output = Some(frame);
        }
        let dialog = h.app.plugin_menu.dialog().unwrap();
        assert!(dialog.running.is_none());
        assert!(dialog.ready(&h.app).is_ok(), "Run stays enabled while the hook waits");
        assert!(crate::scenario::handle_at("plugin_dialog:field:width").is_some(), "fields stay interactive");
        let expected = if stage == "before" { 1.0 } else { 24.0 };
        assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["width"], expected);
        let output = output.unwrap();
        let image = ui_kit::test_raster(&h.ctx, output.shapes, output.pixels_per_point, h.ctx.viewport_rect(), &textures);
        assert_eq!(image.dimensions(), (1280, 720));
        image.save(dir.join(format!("{stage}.png"))).unwrap();
        if stage == "before" {
            send.send(Ok(reply.clone())).unwrap();
        }
    }
}

#[test]
fn conditional_dialog_dropdown_renders_before_and_after_switching() {
    fn paint(
        h: &mut crate::scenario::Harness,
        textures: &mut std::collections::HashMap<egui::TextureId, egui::ColorImage>,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = h.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))),
                time: Some(h.ctx.cumulative_frame_nr() as f64 / 15.0),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| {
                h.app.logic(ui.ctx());
                h.app.ui(ui);
            },
        );
        ui_kit::test_textures_update(textures, &output.textures_delta);
        output.textures_delta.clear();
        output
    }
    fn click(h: &mut crate::scenario::Harness, textures: &mut std::collections::HashMap<egui::TextureId, egui::ColorImage>, key: &str) {
        let at = crate::scenario::handle_at(key).unwrap_or_else(|| panic!("missing {key}"));
        for pressed in [true, false] {
            paint(
                h,
                textures,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
        paint(h, textures, vec![]);
    }
    let mut h = crate::scenario::Harness::new();
    let mut command = entry("cabinet", "edit", json!([{"kind":"menu","name":"Cabinet"}]));
    command.input_schema = cabinet_schema();
    h.app.plugin_menu.listed = vec![listed("cabinet", vec![command])];
    h.app.plugin_menu.loaded = true;
    h.app.home.open = false;
    open_entry(&mut h.app, 0, 0);
    h.app.plugin_menu.dialogs.last_mut().unwrap().preview = false;
    h.ctx = crate::fonts::test_context();
    solidcraft_theme::apply(&h.ctx);
    let mut textures = std::collections::HashMap::new();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/plugin-conditional-fields");
    std::fs::create_dir_all(&dir).unwrap();
    for stage in ["before", "after"] {
        for _ in 0..4 {
            paint(&mut h, &mut textures, vec![]);
        }
        let output = paint(&mut h, &mut textures, vec![]);
        let sent = h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap();
        let expected = if stage == "before" { "shelf_count" } else { "drawer_count" };
        let hidden = if stage == "before" { "drawer_count" } else { "shelf_count" };
        assert!(sent.get(expected).is_some() && sent.get(hidden).is_none());
        assert!(crate::scenario::handle_at(&format!("plugin_dialog:field:{expected}")).is_some());
        assert!(
            crate::test_support::text_shapes(&output).any(|text| text.galley.text() == if stage == "before" { "Shelves *" } else { "Drawers *" })
        );
        let image = ui_kit::test_raster(&h.ctx, output.shapes, output.pixels_per_point, h.ctx.viewport_rect(), &textures);
        assert_eq!(image.dimensions(), (1280, 720));
        image.save(dir.join(format!("{stage}.png"))).unwrap();
        if stage == "before" {
            click(&mut h, &mut textures, "plugin_dialog:field:storage");
            click(&mut h, &mut textures, "dialog:Inside *:Drawers");
        }
    }
    click(&mut h, &mut textures, "plugin_dialog:field:has_back");
    let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
    assert!(!fields.iter().find(|(f, _)| f.key == "back_thickness").unwrap().0.enabled);
    let before = fields.iter().find(|(f, _)| f.key == "back_thickness").unwrap().1.clone();
    click(&mut h, &mut textures, "plugin_dialog:field:back_thickness");
    assert_eq!(h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap().iter().find(|(f, _)| f.key == "back_thickness").unwrap().1, before);
    for _ in 0..3 {
        let mut output = h.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                focused: true,
                ..Default::default()
            },
            |ui| {
                h.app.logic(ui.ctx());
                h.app.ui(ui);
            },
        );
        output.textures_delta.clear();
    }
    for key in ["plugin_dialog:field:storage", "plugin_dialog:field:length", "plugin_dialog:run", "plugin_dialog:cancel"] {
        assert!(h.ctx.viewport_rect().contains(crate::scenario::handle_at(key).unwrap()), "{key} must fit a smaller window");
    }

    h.app.plugin_menu.dialogs.clear();
    open_form(&mut h.app, "cabinet", "cabinet", &json!({"storage":"drawers","size":"small"})).unwrap();
    for _ in 0..4 {
        paint(&mut h, &mut textures, vec![]);
    }
    let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
    assert_eq!(fields.iter().find(|(field, _)| field.key == "size").unwrap().1, Input::Choice(Some(0)));
    assert_eq!(params(fields), Err(Problem::Invalid("Size".into())));
    click(&mut h, &mut textures, "plugin_dialog:field:size");
    click(&mut h, &mut textures, "dialog:Size:large");
    let fields = h.app.plugin_menu.dialog().unwrap().fields.as_ref().unwrap();
    assert_eq!(params(fields).unwrap()["size"], "large", "an explicit dropdown selection corrects the unavailable stored choice");
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
fn conditional_selection_fields_stop_picking_when_hidden_or_disabled() {
    let mut schema = pick_schema();
    schema["properties"]["pick"] = json!({"type":"boolean","default":true});
    schema["properties"]["edges"]["x-solidcraft-visible-when"] = json!({"field":"pick","eq":true});
    schema["properties"]["body"]["x-solidcraft-enabled-when"] = json!({"field":"pick","eq":true});
    let mut command = entry("pick", "edit", json!([{"kind":"menu","name":"Picks"}]));
    command.input_schema = schema;
    let mut h = crate::scenario::Harness::new();
    h.app.plugin_menu.listed = vec![listed("picks", vec![command])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.preview = false;
    let id = dialog.id;
    assert_eq!(picking(&h.app), Some(id));
    set_input(h.app.plugin_menu.dialogs.last_mut().unwrap().fields.as_mut().unwrap(), "pick", Input::Boolean(false));
    h.frames(2);
    assert_eq!(picking(&h.app), None);
    assert!(!start_picking(&mut h.app, id, 0), "hidden field cannot pick");
    assert!(!start_picking(&mut h.app, id, 1), "disabled field cannot pick");
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
        swatches: Default::default(),
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

#[test]
fn importer_menu_rows_use_the_same_entry_icon_for_single_entries_and_submenus() {
    use crate::app_bar::FileRow;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let importer = |name: &str| {
        serde_json::from_value(json!({"name":name,"kind":"importer","extensions":["sample"],
            "input_schema":{"type":"object","x-solidcraft-icon":"format.svg"}}))
        .unwrap()
    };
    let mut plugin = listed("imports", vec![importer("convert")]);
    plugin.images.insert("format.svg".into(), "checked-format-icon".into());
    app.plugin_menu.listed = vec![plugin];
    assert!(matches!(&rows(&app)[3], FileRow::Item(item) if item.icon == "checked-format-icon"));
    app.plugin_menu.listed[0].entries.push(importer("other"));
    let menu = rows(&app);
    let FileRow::Sub { rows, .. } = &menu[3] else { panic!("several importers have a submenu") };
    assert!(rows.iter().all(|row| matches!(row, FileRow::Item(item) if item.icon == "checked-format-icon")));
    app.plugin_menu.listed[0].images.clear();
    let menu = super::rows(&app);
    let FileRow::Sub { rows, .. } = &menu[3] else { panic!("several importers have a submenu") };
    assert!(rows.iter().all(|row| matches!(row, FileRow::Item(item) if item.icon == crate::plugin_images::GENERIC)));
    app.plugin_menu.listed[0].entries.truncate(1);
    app.plugin_menu.listed[0].entries[0].input_schema = json!({"type":"object"});
    assert!(matches!(&super::rows(&app)[3], FileRow::Item(item) if item.icon.is_empty()), "no annotation keeps the old menu row");
    assert_eq!(ribbon_buttons(&app)[0].icon, crate::plugin_images::GENERIC);
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

/// A directory field gets Browse when the host has a folder picker, and no button otherwise.
/// The picked folder is the value sent to the plugin. A plain string field stays a text box.
/// Run refuses a folder outside the install grant, and the grant is unchanged. Paste still
/// fills either field.
#[test]
fn a_directory_field_browses_a_folder_and_an_outside_folder_stays_ungranted() {
    let schema = json!({"type":"object","properties":{
        "folder":{"type":"string","title":"Folder","format":"directory"},
        "note":{"type":"string","title":"Note"}
    }});
    let parsed = form(&schema).expect("drawn");
    assert!(matches!(parsed[0].0.kind, Kind::Text { directory: true, .. }), "format directory is remembered");
    assert!(matches!(parsed[1].0.kind, Kind::Text { directory: false, .. }));

    let mut h = crate::scenario::Harness::new();
    let granted = h.dir.join("granted");
    let outside = h.dir.join("outside");
    std::fs::create_dir_all(&granted).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let mut command = entry("export", "edit", json!([]));
    command.input_schema = schema;
    h.app.plugin_menu.listed = vec![listed("files", vec![command])];
    let grants = solidcraft_engine::plugin::Grants {
        folders: vec![solidcraft_engine::plugin::FileGrant { name: "work".into(), folder: granted.clone(), read: true, write: true }],
        ..Default::default()
    };
    h.app.session.plugin_state.grants.insert("files".into(), grants.clone());
    h.app.preferences.plugin_grants.insert("files".into(), grants);
    open_entry(&mut h.app, 0, 0);
    h.frames(2);
    let browse = h.app.tr("ui.plugin_dialog_browse").to_owned();
    assert!(h.widgets(&browse).is_empty(), "without a folder picker, Browse is not shown");
    assert!(!h.widgets("Folder").is_empty(), "the folder field stays a text box");
    let folder_text = |h: &crate::scenario::Harness| match &h
        .app
        .plugin_menu
        .dialog()
        .unwrap()
        .fields
        .as_ref()
        .unwrap()
        .iter()
        .find(|(f, _)| f.key == "folder")
        .unwrap()
        .1
    {
        Input::Text(text) => text.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(folder_text(&h), "", "the text box starts empty");

    let picked = outside.to_string_lossy().into_owned();
    let answer = picked.clone();
    h.app.services.pick_folder = Some(Box::new(move |_: &str| Box::pin(std::future::ready(Some(answer.clone())))));
    h.frames(2);
    assert_eq!(h.widgets(&browse).len(), 1, "only the directory field has Browse");
    let y = |label: &str| h.widgets(label)[0][1];
    assert!((y(&browse) - y("Folder")).abs() < (y(&browse) - y("Note")).abs(), "Browse sits on the folder row, not the plain string");
    let grants = h.app.session.plugin_state.grants["files"].folders.clone();
    h.step(&json!({"widget": browse})).unwrap();
    h.frames(2);
    assert_eq!(folder_text(&h), picked, "the picked folder is the field value");
    assert!(!h.widgets(&picked).is_empty(), "the text box shows the picked folder, so paste can replace it");
    let fields = h.app.plugin_menu.dialog().unwrap().fields.clone().unwrap();
    assert_eq!(params(&fields), Ok(json!({"folder": picked})), "that path is the value sent to the plugin");
    let err = run(&mut h.app).unwrap_err();
    assert!(err.contains("file path has no applicable folder grant"), "{err}");
    assert!(h.app.control_held.is_empty(), "Run does not go ahead with a folder outside the grant");
    assert_eq!(h.app.session.plugin_state.grants["files"].folders, grants, "picking a folder does not widen the install grant");

    let allowed = granted.to_string_lossy().into_owned();
    {
        let fields = h.app.plugin_menu.dialogs.last_mut().unwrap().fields.as_mut().unwrap();
        fields.iter_mut().find(|(field, _)| field.key == "folder").unwrap().1 = Input::Text(allowed.clone());
        fields.iter_mut().find(|(field, _)| field.key == "note").unwrap().1 = Input::Text("pasted".into());
    }
    let sent = json!({"folder": allowed, "note": "pasted"});
    assert_eq!(
        h.app.plugin_menu.dialog().unwrap().ready(&h.app).as_ref().unwrap(),
        &sent,
        "a pasted path inside the grant, and a pasted string, are sent"
    );
    run(&mut h.app).unwrap();
    let request = h.app.control_held.back().expect("the allowed folder is sent to the plugin");
    assert_eq!(request.params["params"]["params"], sent);
}

#[test]
fn prefilled_directory_values_keep_extras_and_require_the_install_grant() {
    let mut h = crate::scenario::Harness::new();
    let granted = h.dir.join("granted");
    let outside = h.dir.join("outside");
    std::fs::create_dir_all(&granted).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let mut command = entry("export", "edit", json!([]));
    command.input_schema = json!({"type":"object","additionalProperties":true,"properties":{
        "folder":{"type":"string","format":"directory"}
    }});
    h.app.plugin_menu.listed = vec![listed("files", vec![command])];
    // Keep the injected registry when open_form ensures installed entries are listed.
    h.app.plugin_menu.loaded = true;
    let grants = h.app.session.plugin_state.grants.entry("files".into()).or_default();
    grants.folders.push(solidcraft_engine::plugin::FileGrant { name: "work".into(), folder: granted.clone(), read: true, write: true });
    open_form(&mut h.app, "files", "export", &json!({"folder":outside.to_string_lossy(),"operation_id":"saved"})).unwrap();
    checked_prefill(&h.app);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert!(!dialog.preview && !dialog.preview_drawn, "a stored form starts without a drawn preview");
    assert!(dialog.ready(&h.app).unwrap_err().contains("file path has no applicable folder grant"));
    let fields = h.app.plugin_menu.dialogs.last_mut().unwrap().fields.as_mut().unwrap();
    set_input(fields, "folder", Input::Text(granted.to_string_lossy().into_owned()));
    checked_prefill(&h.app);
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app), Ok(json!({"folder":granted.to_string_lossy(),"operation_id":"saved"})));
    assert_eq!(h.app.session.plugin_state.grants["files"].folders.len(), 1, "opening a stored folder adds no grant");
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

    #[test]
    fn panel_row_click_runs_on_the_command_worker_and_refreshes_its_data() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let folder = h.dir.join("sample");
        std::fs::create_dir_all(&folder).unwrap();
        let manifest = json!({"id":"sample","name":"Sample","version":"1","capabilities":["commands@0.2","ui@0.1"],"runtime":{
            "module":"plugin.wasm","commands":["inspect.measure"],"entries":[{"name":"make","effect":"read","input_schema":{
                "type":"object","properties":{"panel":{"type":"string"},"event":{"type":"string"},"path":{"type":"string"},"row":{"type":["string","null"]},"value":{"type":["object","string","null"]}}
            }}],"limits":{"fuel":1000000,"memory_mib":2,"deadline_ms":5000},
            "ui":{"ui_version":1,"panels":[{"id":"jobs","title":"Jobs","zones":["bottom"],"layout":{"root":0,"nodes":[{"widget":{"table":{
                "bind":"/jobs","row_id":"id","columns":[{"key":"state","label":"state","unit":null}],"select":"single","on_select":"make","on_edit":"make"
            }}}]}}],"workspaces":[]}
        }});
        let (package, version) = solidcraft_engine::plugin::API.split_once('@').unwrap();
        let source = include_str!("../../craft-plugin-wasm/src/ui_fixture.wat")
            .replace("__PACKAGE__", package)
            .replace("__VERSION__", version)
            .replace("__COMMAND__", "inspect.measure")
            .replace("__COMMAND_LEN__", "15")
            .replace("__ARGS__", "{}")
            .replace("__ARGS_LEN__", "2");
        std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(folder.join("plugin.wasm"), wat::parse_str(source).unwrap()).unwrap();
        h.app.session.plugin_state.plugins_dir = Some(h.dir.clone());
        let inspected = h.app.run("plugin.inspect", json!({"path":folder.to_string_lossy()})).unwrap();
        let (initial, _) = h.call_queued("engine.execute", json!({"command":"sample.make","params":{"approval":inspected["approval"],"params":{}}}));
        assert_eq!(initial["ok"], true, "{initial}");
        h.frames(3);
        assert_eq!(h.app.session.plugin_panels()[0].data.get("/jobs/0/state"), Some(&solidcraft_engine::plugin::ui::UiValue::Text("ready".into())));
        let at = crate::scenario::rect_of("plugin-panel:sample:jobs:row:op1").expect("a real shared-host table row").center();
        for pressed in [true, false] {
            h.frame_with(crate::test_support::pointer(at, pressed));
        }
        settle(&mut h);
        h.frames(2);
        assert_eq!(
            h.app.session.plugin_panels()[0].data.get("/jobs/0/state"),
            Some(&solidcraft_engine::plugin::ui::UiValue::Text("posted".into())),
            "clicks must reach the worker and adopt a later set-data"
        );
        assert!(h.app.session.doc.features.is_empty(), "read actions never edit the design");
        // Keep the synthetic replacement below until the queued action has been inspected.
        h.app.plugin_menu.loaded = true;
        let pinned = h.app.session.plugin_panels()[0].clone();
        crate::panel_runtime::queue_selection(
            &h.app,
            "sample".into(),
            pinned.approval.clone(),
            "make".into(),
            solidcraft_engine::plugin::ui::UiEvent {
                panel: "jobs".into(),
                event: solidcraft_engine::plugin::ui::EventKind::Select,
                path: "/jobs".into(),
                row: Some("op1".into()),
                value: None,
            },
        );
        h.app.session.plugin_state.panels.get_mut("sample").unwrap().approval = "0".repeat(64);
        h.frames(1);
        assert!(h.app.plugin_menu.panel_runs.is_empty(), "a stale click never starts a worker under a replacement package's hash");
        assert_eq!(h.app.session.plugin_panels()[0].data, pinned.data, "a discarded action leaves panel data alone");
        h.app.session.plugin_state.panels.get_mut("sample").unwrap().approval = pinned.approval;

        let id = h
            .app
            .universal_panel_frame()
            .unwrap()
            .layout
            .sections
            .iter()
            .find(|section| section.instance.0.starts_with("plugin."))
            .unwrap()
            .instance
            .clone();
        h.app
            .dispatch_panel_layout(solidcraft_engine::panels::LayoutAction::Apply {
                expected_revision: None,
                mutations: vec![solidcraft_engine::panels::Mutation::Close { instance: id.clone() }],
            })
            .unwrap();
        h.frames(1);
        assert!(h.app.session.plugin_panels()[0].visible, "a manual close is workspace placement, not guest state");
        h.app.session.plugin_state.panels.get_mut("sample").unwrap().visibility_revision.insert("jobs".into(), 1);
        h.frames(2);
        assert!(
            h.app.universal_panel_frame().unwrap().layout.sections.iter().any(|section| section.instance == id),
            "a later show-panel(true) reopens the panel"
        );
        use solidcraft_engine::panels::{Destination, LayoutAction, Mutation, Point, SectionId, SectionLocation, Size};
        let location = SectionLocation::Floating { origin: Point { x: 420.0, y: 180.0 } };
        h.app
            .dispatch_panel_layout(LayoutAction::Apply {
                expected_revision: None,
                mutations: vec![Mutation::Move {
                    instance: id.clone(),
                    destination: Destination::NewSection {
                        section: SectionId::new("plugin-floating").unwrap(),
                        index: 0,
                        location: location.clone(),
                        size: Size { width: 400.0, height: 240.0 },
                    },
                }],
            })
            .unwrap();
        h.frames(2);
        h.app.run_panel_command("layout.workspace.select", &json!({"workspace":"solidcraft.review"})).unwrap();
        h.frames(2);
        h.app.run_panel_command("layout.workspace.select", &json!({"workspace":"solidcraft.classic"})).unwrap();
        h.frames(2);
        assert_eq!(
            h.app.panel_runtime().unwrap().layout().layout().containing(&id).unwrap().location,
            location,
            "placement is remembered per workspace"
        );
        {
            let package = h.app.session.plugin_state.panels.get_mut("sample").unwrap();
            package.visible.insert("jobs".into(), false);
            package.visibility_revision.insert("jobs".into(), 2);
        }
        h.frames(2);
        assert!(!h.app.universal_panel_frame().unwrap().layout.sections.iter().any(|section| section.instance == id));
        h.app.run_panel_command("layout.workspace.select", &json!({"workspace":"solidcraft.review"})).unwrap();
        h.frames(2);
        assert!(
            h.app.universal_panel_frame().unwrap().layout.sections.iter().any(|section| section.instance == id),
            "a consumed hide request does not replay in another workspace"
        );
        h.app.run_panel_command("layout.workspace.select", &json!({"workspace":"solidcraft.classic"})).unwrap();
        h.frames(2);
        let saved = crate::panel_runtime::preferences_value(&h.app).unwrap();
        let mut restarted = crate::scenario::Harness::new();
        restarted.app.home.open = false;
        restarted.app.disable_panel_runtime().unwrap();
        restarted.app.session.plugin_state.plugins_dir = Some(h.dir.clone());
        crate::panel_runtime::load_preferences(&mut restarted.app, &json!({"panel_layout":saved}));
        restarted.frames(3);
        let restored = restarted.app.panel_runtime().unwrap().layout().layout().instance(&id).unwrap();
        assert!(!restored.requested_open, "an inert default declaration honours a restored closed instance");
        assert_eq!(restored.memory.location, location, "inert plugin registration precedes saved layout restoration");
        restarted.app.session.plugin_state.panels.get_mut("sample").unwrap().visibility_revision.insert("jobs".into(), 1);
        restarted.frames(2);
        assert_eq!(
            restarted.app.panel_runtime().unwrap().layout().layout().containing(&id).unwrap().location,
            location,
            "a new explicit show request reopens a restored closed instance"
        );
    }

    fn settle(h: &mut crate::scenario::Harness) {
        for _ in 0..500 {
            h.frames(1);
            if h.app.plugin_menu.dialog().is_none_or(|d| d.running.is_none()) && h.app.plugin_menu.panel_runs.is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the run did not finish");
    }

    #[test]
    fn interactive_guest_import_opens_the_same_prefilled_form_after_success() {
        let root = package("guest-form");
        let dir = root.join("boxes");
        let (name, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
        let original = include_str!("plugin_menu_fixture.wat").replace("PLUGIN_PACKAGE", name).replace("PLUGIN_VERSION", version);
        let source = original.replace("/commands@","/forms@").replace("\"run\"","\"open\"").replace("(param \"command\" string)","(param \"entry\" string)").replace("(param \"args\" 1)","(param \"params\" 1)")
            .replace("(result 1 (error 3))","(result (error 3))")
            .replace("i32.const 9","i32.const 4").replace("solid.box","edit")
            .replace("      call $run\n      i32.const 0", "      call $run\n      i32.const 0 i32.const 0 i32.store\n      i32.const 4 i32.const 2048 i32.store\n      i32.const 8 i32.const 2 i32.store\n      i32.const 0")
            .replacen("    (@producers", "    (data (i32.const 2048) \"{}\")\n    (@producers", 1);
        std::fs::write(dir.join("plugin.wasm"), wat::parse_str(&source).unwrap()).unwrap();
        let mut m: Value = serde_json::from_slice(&std::fs::read(dir.join("plugin.json")).unwrap()).unwrap();
        let mut edit = m["runtime"]["entries"][0].clone();
        edit["name"] = json!("edit");
        let mut choose = edit.clone();
        choose["name"] = json!("choose");
        choose["effect"] = json!("read");
        m["capabilities"] = json!(["commands@0.2", "forms@0.1"]);
        m["runtime"]["entries"] = json!([choose, edit]);
        m["runtime"]["commands"] = json!([]);
        std::fs::write(dir.join("plugin.json"), m.to_string()).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        h.app.plugin_menu.loaded = true;
        let plugin = h.app.plugin_menu.listed[0].clone();
        let mut preview = h.app.session.scratch();
        dry_run(&mut preview, "boxes");
        let preview_reply =
            preview.execute("plugin.run", &json!({"path":plugin.path,"approval":plugin.approval,"entry":"choose","params":{}})).unwrap();
        assert_eq!(preview_reply["forms"].as_array().unwrap().len(), 1);
        assert!(h.app.plugin_menu.dialogs.is_empty(), "preview requests stay in the discarded scratch reply");
        assert_eq!(h.app.session.doc, preview.doc, "this preview commits nothing");
        let refused =
            h.app.session.execute("plugin.run", &json!({"path":plugin.path,"approval":plugin.approval,"entry":"choose","params":{}})).unwrap_err();
        assert!(matches!(refused.host_error(), Some(solidcraft_engine::plugin::HostError::InteractiveRequired { .. })), "{refused}");
        let before = h.app.session.doc.clone();
        open_entry(&mut h.app, 0, 0);
        run(&mut h.app).unwrap();
        settle(&mut h);
        checked_prefill(&h.app);
        let opened = h.app.plugin_menu.dialogs.iter().find(|d| d.entry == "edit").expect("guest form request opens after the read finishes");
        assert!(opened.form_guard.is_some());
        assert_eq!(opened.ready(&h.app).unwrap(), json!({"length":20.0,"width":10.0,"height":3.0}));
        assert_eq!(h.app.session.doc, before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prefilled_form_apply_revalidates_changes_and_cancel_keeps_design_equal() {
        let root = package("prefill-apply");
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        h.app.plugin_menu.loaded = true;
        let values = json!({"length":30,"width":10,"height":3});
        open_form(&mut h.app, "boxes", "make", &values).unwrap();
        h.app.plugin_menu.dialogs.last_mut().unwrap().preview = false;
        h.app.run("solid.box", json!({"length":1,"width":1,"height":1})).unwrap();
        checked_prefill(&h.app);
        let newer = h.app.session.doc.clone();
        run(&mut h.app).unwrap();
        settle(&mut h);
        assert_eq!(h.app.session.doc, newer, "stale Apply does not run");
        assert!(h.app.plugin_menu.dialog().unwrap().changed);
        let checked_revision = h.app.plugin_menu.dialog().unwrap().form_guard.as_ref().unwrap().revision;
        assert_eq!(checked_revision, h.app.session.design_revision(), "validation and the live design share one revision");
        let undo = h.app.session.undo.len();
        run(&mut h.app).unwrap();
        settle(&mut h);
        assert!(h.app.plugin_menu.dialog().is_none());
        assert_eq!(h.app.session.undo.len(), undo + 1);
        let after = h.app.session.doc.clone();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc, newer);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc, after);
        open_form(&mut h.app, "boxes", "make", &values).unwrap();
        h.app.plugin_menu.dialogs.last_mut().unwrap().preview = false;
        h.frames(3);
        let at = crate::scenario::handle_at("plugin_dialog:cancel").unwrap();
        h.step(&json!({"click":[at.x,at.y]})).unwrap();
        assert_eq!(h.app.session.doc, after, "Cancel leaves the design equal");
        let _ = std::fs::remove_dir_all(root);
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

    /// A package with `home` (a box, then view.home), `edit` (a box and no view command),
    /// `fail`, `place` (a new component moved 100 mm along X), `move` (only the move), and
    /// `both` (move the existing component, then make another box), and `fit` (a box from the
    /// fields, then view.home followed by view.fit).
    fn view_package(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("solidcraft-plugin-view-{}-{tag}", std::process::id()));
        let dir = root.join("frames");
        std::fs::create_dir_all(&dir).unwrap();
        let (name, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
        let wat = include_str!("plugin_view_fixture.wat").replace("PLUGIN_PACKAGE", name).replace("PLUGIN_VERSION", version);
        std::fs::write(dir.join("plugin.wasm"), wat::parse_str(&wat).unwrap()).unwrap();
        let entry = |entry_name: &str, label: &str| json!({"name": entry_name, "effect": "edit", "input_schema": {"type": "object"}, "slots": [{"kind": "menu", "name": label}]});
        let mut fit = entry("fit", "Fit");
        fit["input_schema"] = json!({"type":"object","required":["length","width","height"],"properties":{
            "length":{"type":"number","exclusiveMinimum":0,"default":20},
            "width":{"type":"number","exclusiveMinimum":0,"default":10},
            "height":{"type":"number","exclusiveMinimum":0,"default":3}}});
        let mut sketch = entry("sketch", "Sketch");
        sketch["input_schema"] = json!({"type":"object","required":["solid"],"properties":{
            "solid":{"type":"boolean","default":false}}});
        let mut zero = sketch.clone();
        zero["name"] = json!("zero");
        let manifest = json!({"id":"frames","name":"Frames","version":"0.1.0","capabilities":["commands@0.1"],"runtime":{
            "module":"plugin.wasm",
            "entries":[entry("home", "Home"), entry("edit", "Edit"), entry("fail", "Fail"), entry("place", "Place"), entry("move", "Move"), entry("both", "Both"), fit, sketch, zero, entry("consumed", "Consumed")],
            "commands":["solid.box", "view.fit", "view.home", "component.create", "occurrence.move", "sketch.create", "sketch.rectangle.two_point", "sketch.finish", "sketch.line", "solid.extrude"]}});
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

    /// Moving an existing component adds no material, even before the design has a placement.
    #[test]
    fn a_component_a_run_only_moves_keeps_its_body_colour_in_the_preview() {
        let root = view_package("move-colour");
        let mut h = crate::scenario::Harness::new();
        h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new_body"})).unwrap();
        h.app.preferences.preview_tint_new_faces = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        h.app.plugin_menu.loaded = true;
        assert!(!solidcraft_engine::frames::placed(&h.app.session), "the component starts at its definition");
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        let entry = h.app.plugin_menu.listed[0].entries.iter().position(|entry| entry.name == "move").unwrap();
        open_entry(&mut h.app, 0, entry);
        previewed(&mut h);
        assert!(h.app.preview.active, "the moved box is drawn: {:?}", h.app.preview.error);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc), "the preview leaves the design untouched");
        assert_eq!(h.app.session.undo.len(), undo, "the preview adds no undo step");

        let (_, id, cmds) = preview_commands(&h.app).expect("the dialog is previewed");
        let mut scratch = h.app.session.scratch();
        dry_run(&mut scratch, &id);
        let colors = crate::preview::Colors { tint_new: true, ..crate::preview::Colors::from_tokens(&solidcraft_theme::Tokens::get()) };
        assert_ne!(colors.body, colors.added);
        assert_ne!(colors.body, colors.new_body);
        let built = crate::preview::compute(&scratch, &cmds, colors).unwrap();
        let (vertices, rest) = built.scene.tris.as_chunks::<{ crate::gpu::TRI_SIZE }>();
        assert!(rest.is_empty() && !vertices.is_empty(), "the box has preview triangles");
        let xs = vertices.iter().map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]));
        let (lo, hi) = xs.fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        assert_eq!((lo.round(), hi.round()), (100.0, 110.0), "the preview moves the existing box 100 mm along X");
        assert!(!built.scene.ghost.is_empty(), "the old placement remains visible as a ghost");
        let count = |colour| {
            vertices
                .iter()
                .filter(|v| {
                    let index = u32::from_le_bytes([v[24], v[25], v[26], v[27]]);
                    built.scene.palette.get(crate::gpu::PaletteIndex(index)) == Some(colour)
                })
                .count()
        };
        if let Some(path) = std::env::var_os("SOLIDCRAFT_PLUGIN_PREVIEW_SHOT") {
            const SIZE: u32 = 512;
            h.app.cam.target = solidcore_record::Vec3::new(105.0, 5.0, 5.0);
            h.app.cam.distance = 35.0;
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE as f32, SIZE as f32));
            let proj = crate::viewport::projection(&h.app, rect);
            let e = proj.cam.eye().to_f32();
            let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            let pixels = crate::gpu::render_offscreen_preview(
                crate::gpu::GpuScene::default(),
                built.scene.clone(),
                None,
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
            )
            .expect("a GPU adapter is required for the requested preview capture");
            image::RgbaImage::from_raw(SIZE, SIZE, pixels).unwrap().save(path).unwrap();
        }
        let counts = (count(colors.body), count(colors.added), count(colors.new_body));
        let _ = std::fs::remove_dir_all(root);
        assert_eq!(counts, (vertices.len(), 0, 0), "moving an existing body adds no new surface or body");
    }

    /// Each body keeps its own preview role when a plugin moves a part and makes material.
    #[test]
    fn a_run_tints_only_new_bodies_even_when_an_existing_component_was_already_placed() {
        for (start, entry_name) in [(0, "both"), (20, "move"), (20, "both")] {
            let root = view_package(&format!("mixed-colour-{start}-{entry_name}"));
            let mut h = crate::scenario::Harness::new();
            h.app.run("component.create", json!({"name": "Part", "grounded": false})).unwrap();
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new_body"})).unwrap();
            if start != 0 {
                h.app.run("occurrence.move", json!({"occurrence": "Part:1", "translate": [start, 0, 0]})).unwrap();
            }
            assert_eq!(solidcraft_engine::frames::placed(&h.app.session), start != 0);
            h.app.plugin_menu.listed = list(&h.app, Some(&root));
            h.app.plugin_menu.loaded = true;
            let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
            let entry = h.app.plugin_menu.listed[0].entries.iter().position(|entry| entry.name == entry_name).unwrap();
            open_entry(&mut h.app, 0, entry);
            let (_, id, cmds) = preview_commands(&h.app).expect("the dialog is previewed");
            let mut scratch = h.app.session.scratch();
            dry_run(&mut scratch, &id);
            let colors = crate::preview::Colors { tint_new: true, ..crate::preview::Colors::from_tokens(&solidcraft_theme::Tokens::get()) };
            assert_ne!(colors.body, colors.added);
            assert_ne!(colors.body, colors.new_body);
            let built = crate::preview::compute(&scratch, &cmds, colors).unwrap();
            assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc), "the preview leaves the design untouched");
            assert_eq!(h.app.session.undo.len(), undo, "the preview adds no undo step");
            assert!(built.scene.xray.is_empty(), "moving and making bodies removes no material");
            let (vertices, rest) = built.scene.tris.as_chunks::<{ crate::gpu::TRI_SIZE }>();
            assert!(rest.is_empty() && !vertices.is_empty());
            let mut moved = Vec::new();
            let mut made = Vec::new();
            for vertex in vertices {
                let index = u32::from_le_bytes([vertex[24], vertex[25], vertex[26], vertex[27]]);
                let colour = built.scene.palette.get(crate::gpu::PaletteIndex(index));
                let x = f32::from_le_bytes([vertex[0], vertex[1], vertex[2], vertex[3]]);
                if colour == Some(colors.body) {
                    moved.push(x);
                } else {
                    assert_eq!(colour, Some(colors.new_body), "{entry_name} from {start}: no existing surface is new");
                    made.push(x);
                }
            }
            let span = |xs: &[f32]| xs.iter().copied().fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            // The original box spans 0..10; its old placement and the plugin's 100 mm move add.
            assert_eq!(span(&moved), ((start + 100) as f32, (start + 110) as f32), "{entry_name}: the existing box stays body-coloured");
            if entry_name == "both" {
                // solid.box takes world coordinates, even inside the now-moved component.
                assert_eq!(span(&made), (20.0, 30.0), "only the second box is new material");
            } else {
                assert!(made.is_empty(), "a move alone makes no new body");
            }
            let _ = std::fs::remove_dir_all(root);
        }
    }

    fn same_camera(a: &solidcraft_viewport::Camera, b: &solidcraft_viewport::Camera) -> bool {
        let near =
            |p: solidcore_record::Vec3, q: solidcore_record::Vec3| (p.x - q.x).abs() < 1e-6 && (p.y - q.y).abs() < 1e-6 && (p.z - q.z).abs() < 1e-6;
        near(a.target, b.target) && (a.distance - b.distance).abs() < 1e-6 && (a.yaw - b.yaw).abs() < 1e-9 && (a.pitch - b.pitch).abs() < 1e-9
    }

    #[test]
    fn the_first_plugin_preview_fits_once_and_reopening_fits_again() {
        let root = view_package("preview-fit");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.plugin_menu.loaded = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let fit = h.app.plugin_menu.listed[0].entries.iter().position(|e| e.name == "fit").unwrap();
        h.app.cam.set_view(solidcraft_viewport::StandardView::Front);
        h.app.cam.distance = 800.0;
        let parked = h.app.cam;
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        open_entry(&mut h.app, 0, fit);
        previewed(&mut h);
        assert!(h.app.preview.active, "the box is drawn: {:?}", h.app.preview.error);
        let bounds = solidcraft_engine::geom::Aabb3 { min: solidcore_record::Vec3::ZERO, max: solidcore_record::Vec3::new(20.0, 10.0, 3.0) };
        let expected = h.app.fitted_bounds(parked, &bounds);
        assert!(!same_camera(&parked, &h.app.cam), "the first preview moves the camera to its result");
        assert!(same_camera(&expected, &h.app.cam), "the last command is Fit, retaining the Front direction");
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);

        // Navigation stays available while the dialog is open, through the shared camera path.
        h.step(&json!({"call":"ui.scroll","params":{"x":800,"y":600,"delta":-100}})).unwrap();
        h.frames(20);
        let moved = h.app.cam;
        assert!(!same_camera(&expected, &moved), "the user can zoom the preview");
        let key = h.app.preview.key;
        h.app.plugin_menu.dialogs[0].fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "length").unwrap().1 = Input::Number(30.0);
        previewed(&mut h);
        assert_ne!(h.app.preview.key, key, "the changed field produced a new preview");
        assert_eq!(h.app.preview.bounds.max.x.round(), 30.0);
        assert!(same_camera(&moved, &h.app.cam), "later previews preserve the user's view");

        h.step(&json!({"click":{"handle":"plugin_dialog:cancel"}})).unwrap();
        h.frames(2);
        assert!(h.app.plugin_menu.dialogs.is_empty());
        open_entry(&mut h.app, 0, fit);
        previewed(&mut h);
        assert!(!same_camera(&moved, &h.app.cam), "a reopened dialog fits its first preview again");
        assert!(same_camera(&h.app.fitted_bounds(moved, &bounds), &h.app.cam));

        // OK still applies the last view command even after the preview already applied it.
        h.app.cancel_view_animation();
        h.app.cam = parked;
        run(&mut h.app).unwrap();
        settle(&mut h);
        h.frames(2);
        assert_eq!(h.app.session.doc.features.len(), 1);
        assert!(!same_camera(&parked, &h.app.cam));
        assert!(same_camera(&h.app.fitted(parked), &h.app.cam), "OK still fits the committed result");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_first_plugin_preview_applies_home_and_a_preview_without_a_view_command_leaves_the_camera() {
        let root = view_package("preview-home");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.plugin_menu.loaded = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let home = h.app.plugin_menu.listed[0].entries.iter().position(|e| e.name == "home").unwrap();
        let edit = h.app.plugin_menu.listed[0].entries.iter().position(|e| e.name == "edit").unwrap();
        h.app.cam.set_view(solidcraft_viewport::StandardView::Front);
        h.app.cam.distance = 800.0;
        let parked = h.app.cam;
        open_entry(&mut h.app, 0, edit);
        previewed(&mut h);
        assert!(h.app.preview.active);
        assert!(same_camera(&parked, &h.app.cam), "a preview with no view command changes nothing");
        open_entry(&mut h.app, 0, home);
        previewed(&mut h);
        assert!(h.app.preview.active);
        let mut iso = parked;
        iso.set_view(solidcraft_viewport::StandardView::Iso);
        assert_eq!((h.app.cam.yaw, h.app.cam.pitch), (iso.yaw, iso.pitch), "Home turns to the home direction");
        assert!(h.app.cam.distance < parked.distance, "Home fits the previewed box");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_sketch_only_first_plugin_preview_fits_once_and_a_later_solid_keeps_the_camera() {
        let root = view_package("preview-sketch-fit");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.plugin_menu.loaded = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let sketch = h.app.plugin_menu.listed[0].entries.iter().position(|e| e.name == "sketch").unwrap();
        h.app.cam.set_view(solidcraft_viewport::StandardView::Top);
        h.app.cam.distance = 800.0;
        let parked = h.app.cam;
        open_entry(&mut h.app, 0, sketch);
        previewed(&mut h);
        assert!(h.app.preview.active, "the sketch preview is ready: {:?}", h.app.preview.error);
        assert_eq!(h.app.preview.sketches.len(), 1);
        assert_eq!(h.app.preview.bounds.max.z, 0.0, "the first result is a planar sketch");
        // The rectangle spans (100,20)..(120,30) in the XY plane, away from the origin.
        let bounds =
            solidcraft_engine::geom::Aabb3 { min: solidcore_record::Vec3::new(100.0, 20.0, 0.0), max: solidcore_record::Vec3::new(120.0, 30.0, 0.0) };
        let expected = h.app.fitted_bounds(parked, &bounds);
        assert!(same_camera(&expected, &h.app.cam), "Fit frames the preview's sketch geometry");
        assert!(h.app.plugin_menu.dialogs[0].preview_drawn, "the first sketch result consumes the view action");
        assert!(h.app.session.doc.features.is_empty(), "the sketch is only previewed");

        h.step(&json!({"call":"ui.scroll","params":{"x":800,"y":600,"delta":-100}})).unwrap();
        h.frames(20);
        let moved = h.app.cam;
        assert!(!same_camera(&expected, &moved), "navigation is available after the sketch preview");
        let key = h.app.preview.key;
        h.app.plugin_menu.dialogs[0].fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "solid").unwrap().1 = Input::Boolean(true);
        previewed(&mut h);
        assert_ne!(h.app.preview.key, key);
        assert!(h.app.preview.active, "the solid preview is ready: {:?}", h.app.preview.error);
        assert_eq!(h.app.preview.bounds.max.z.round(), 10.0, "the field change produces a solid");
        assert!(same_camera(&moved, &h.app.cam), "a later solid leaves the user's camera alone");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_unchanged_visible_sketch_is_included_in_the_first_plugin_preview_fit() {
        let root = view_package("preview-unchanged-sketch-fit");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[100,20],"p1":[120,30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let sketch = h.app.session.world_state().unwrap().sketches[0].feature;
        assert!(solidcraft_engine::view::sketch_shown(&h.app.session, sketch));
        h.app.plugin_menu.loaded = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let fit = h.app.plugin_menu.listed[0].entries.iter().position(|e| e.name == "fit").unwrap();
        h.app.cam.set_view(solidcraft_viewport::StandardView::Top);
        h.app.cam.distance = 800.0;
        let parked = h.app.cam;
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        open_entry(&mut h.app, 0, fit);
        previewed(&mut h);
        assert!(h.app.preview.active, "the preview is ready: {:?}", h.app.preview.error);
        assert!(h.app.preview.sketches.is_empty(), "the existing sketch is unchanged by the run");
        // The new box spans (0,0,0)..(20,10,3); the shown rectangle extends to (120,30,0).
        let bounds = solidcraft_engine::geom::Aabb3 { min: solidcore_record::Vec3::ZERO, max: solidcore_record::Vec3::new(120.0, 30.0, 3.0) };
        assert_eq!(h.app.preview.bounds, bounds, "framing includes the unchanged shown sketch");
        let expected = h.app.fitted_bounds(parked, &bounds);
        assert!(same_camera(&expected, &h.app.cam), "the first preview fits all shown geometry");
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);

        h.app.cam = parked;
        run(&mut h.app).unwrap();
        settle(&mut h);
        h.frames(2);
        assert!(solidcraft_engine::view::sketch_shown(&h.app.session, sketch));
        assert_eq!(solidcraft_engine::view::bounds(&h.app.session).unwrap(), bounds);
        // The bounds above are the preview's; the dialog has closed, so Fit has the whole viewport.
        let committed = h.app.fitted_bounds(parked, &bounds);
        assert!(same_camera(&committed, &h.app.cam), "preview and committed Fit frame the same geometry");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_sketch_a_plugin_consumes_does_not_widen_its_first_preview_fit() {
        let root = view_package("preview-consumed-sketch-fit");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.plugin_menu.loaded = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let entry = h.app.plugin_menu.listed[0].entries.iter().position(|e| e.name == "consumed").unwrap();
        h.app.cam.set_view(solidcraft_viewport::StandardView::Top);
        h.app.cam.distance = 800.0;
        let parked = h.app.cam;
        open_entry(&mut h.app, 0, entry);
        previewed(&mut h);
        assert!(h.app.preview.active, "the preview is ready: {:?}", h.app.preview.error);
        assert_eq!(h.app.preview.sketches.len(), 1, "the run changed the distant sketch's geometry");
        assert_eq!(
            solidcraft_engine::view::sketch_bounds(&h.app.preview.sketches[0].sketch).max.x,
            120.0,
            "the sketch itself extends past the solid"
        );
        // The rectangle extrudes to (0,0,0)..(10,10,10). Its sketch also has a construction
        // line ending at (120,30), which must not enlarge Fit once the sketch is consumed.
        let bounds =
            solidcraft_engine::geom::Aabb3 { min: solidcore_record::Vec3::new(0.0, 0.0, 0.0), max: solidcore_record::Vec3::new(10.0, 10.0, 10.0) };
        assert_eq!(h.app.preview.bounds, bounds, "only the shown box contributes to framing");
        let expected = h.app.fitted_bounds(parked, &bounds);
        assert!(same_camera(&expected, &h.app.cam), "the first preview fits only shown geometry");
        assert!(h.app.plugin_menu.dialogs[0].preview_drawn);
        assert!(h.app.session.doc.features.is_empty(), "the run is still only previewed");

        h.app.cam = parked;
        run(&mut h.app).unwrap();
        settle(&mut h);
        h.frames(2);
        let sketch = h.app.session.world_state().unwrap().sketches[0].feature;
        assert!(solidcraft_engine::view::sketch_consumed(&h.app.session, sketch), "the extrusion consumes the sketch");
        assert!(!solidcraft_engine::view::sketch_shown(&h.app.session, sketch), "the consumed sketch is hidden");
        assert_eq!(solidcraft_engine::view::bounds(&h.app.session).unwrap(), bounds);
        // The bounds above are the preview's; the dialog has closed, so Fit has the whole viewport.
        let committed = h.app.fitted_bounds(parked, &bounds);
        assert!(same_camera(&committed, &h.app.cam), "preview and committed Fit frame the same geometry");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_empty_first_plugin_preview_applies_home_once_and_a_later_solid_keeps_the_camera() {
        let root = view_package("preview-empty-home");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.plugin_menu.loaded = true;
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        let zero = h.app.plugin_menu.listed[0].entries.iter().position(|e| e.name == "zero").unwrap();
        h.app.cam.set_view(solidcraft_viewport::StandardView::Front);
        h.app.cam.distance = 800.0;
        let mut iso = h.app.cam;
        iso.set_view(solidcraft_viewport::StandardView::Iso);
        open_entry(&mut h.app, 0, zero);
        previewed(&mut h);
        assert!(h.app.preview.active, "the empty result is ready: {:?}", h.app.preview.error);
        assert!(h.app.preview.bounds.is_empty());
        assert!(h.app.plugin_menu.dialogs[0].preview_drawn, "an empty result consumes the first preview");
        assert_eq!((h.app.cam.yaw, h.app.cam.pitch), (iso.yaw, iso.pitch), "Home needs no bounds");
        h.step(&json!({"call":"ui.scroll","params":{"x":800,"y":600,"delta":-100}})).unwrap();
        h.frames(20);
        let moved = h.app.cam;
        h.app.plugin_menu.dialogs[0].fields.as_mut().unwrap().iter_mut().find(|(f, _)| f.key == "solid").unwrap().1 = Input::Boolean(true);
        previewed(&mut h);
        assert!(!h.app.preview.bounds.is_empty(), "the later preview contains a solid");
        assert!(same_camera(&moved, &h.app.cam), "the later solid does not apply Home again");
        let _ = std::fs::remove_dir_all(root);
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

    #[test]
    fn one_run_folder_consent_reaches_the_command_worker_without_changing_its_source_identity() {
        let root = package("folder-consent");
        let path = root.join("boxes/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["runtime"]["entries"][0]["input_schema"]["properties"]["library"] =
            json!({"type":"string","format":"directory","x-solidcraft-grant":"read"});
        std::fs::write(&path, manifest.to_string()).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_menu.listed = list(&h.app, Some(&root));
        open_entry(&mut h.app, 0, 0);
        let id = h.app.plugin_menu.dialog().unwrap().id;
        apply_browsed_folder(&mut h.app, id, "library", root.to_string_lossy().into_owned());
        run(&mut h.app).unwrap();
        settle(&mut h);
        assert!(h.app.plugin_menu.dialogs.is_empty(), "a successful edit must be accepted from its worker");
        assert!(h.app.session.plugin_state.user.dialog_folders.is_none());
        assert!(h.app.session.plugin_state.grants.is_empty() && h.app.preferences.plugin_grants.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn replacing_an_importer_file_invalidates_pending_general_patches_before_reading_the_new_file() {
    let mut h = crate::scenario::Harness::new();
    let mut entry = importing_entry();
    entry.dialog_hook = true;
    entry.input_schema["properties"]["bodies"] = json!({"type":"array","items":{"type":"string"}});
    h.app.session.selection = vec![Sel::Body { name: "prefilled".into() }];
    h.app.plugin_menu.listed = vec![listed("sample", vec![entry])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let id = dialog.id;
    let generation = dialog.hook.as_ref().unwrap().generation;
    let update = solidcraft_engine::plugin::dialog::apply(
        &dialog.hook.as_ref().unwrap().schema,
        &json!({}),
        &Default::default(),
        Some(r#"{"part":{"value":"old file"},"bodies":{"value":["computed"]}}"#),
        false,
    )
    .unwrap();
    assert!(apply_hook_reply(dialog, generation, update.clone()));
    let (send, receive) = std::sync::mpsc::channel();
    dialog.hook.as_mut().unwrap().pending = Some((generation, receive));
    h.app.dialog.as_mut().unwrap().inputs = dialog.selection_inputs();
    select_file(&mut h.app, id, "replacement.smp".into());
    sync_picks(&mut h.app);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert!(dialog.hook.as_ref().unwrap().generation > generation);
    assert!(dialog.file_job.is_none(), "the new file reader waits for the previous hook worker");
    send.send(Ok(update)).unwrap();
    poll_hooks(&mut h.app, &h.ctx);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert!(dialog.hook.as_ref().unwrap().pending.is_none());
    assert!(dialog.file.is_none());
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["part"], "initial", "the old file's patch was discarded");
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["bodies"], json!(["prefilled"]), "the original viewport prefill survives");
}

#[test]
fn importer_file_conditions_and_general_patches_share_defaults_without_overwriting_edits() {
    let schema = json!({"type":"object","properties":{
        "width":{"type":"number","default":1,"x-solidcraft-defaults":[{"when":{"file":"extension","eq":"asm"},"value":6}]},
        "part":{"type":"string","default":"initial"},
        "detail":{"type":"string","default":"shown","x-solidcraft-visible-when":{"file":"extension","eq":"asm"}}
    }});
    let mut h = crate::scenario::Harness::new();
    let mut entry = importing_entry();
    entry.dialog_hook = true;
    entry.input_schema = schema.clone();
    h.app.plugin_menu.listed = vec![listed("sample", vec![entry])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    set_input(dialog.fields.as_mut().unwrap(), "part", Input::Text("chosen".into()));
    dialog.source = Some("parts.asm".into());
    dialog.file_generation = 1;
    let file = solidcraft_engine::plugin::dialog::FileFacts {
        name: "parts.asm".into(),
        extension: "asm".into(),
        size: 42,
        format: "sample.convert".into(),
        generation: 1,
    };
    let bound = solidcraft_engine::plugin::conditions::bind_file_conditions(&schema, Some(&file)).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    dialog.file_job = Some(FileJob { generation: 1, reply: receive });
    send.send(Ok(solidcraft_engine::plugin::ImportDialog { file, schema: bound.clone(), hook: false })).unwrap();
    poll_file(dialog);
    assert_eq!(dialog.hook.as_ref().unwrap().schema, bound);
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["width"], 6.0);
    let update = solidcraft_engine::plugin::dialog::apply(
        &bound,
        &json!({"part":"chosen"}),
        &std::collections::BTreeSet::from(["part".into()]),
        Some(r#"{"width":{"default":24},"part":{"value":"computed"},"detail":{"visible":false,"enabled":false,"hint":"Header detail"}}"#),
        false,
    )
    .unwrap();
    assert!(update.warning.is_none());
    let generation = dialog.hook.as_ref().unwrap().generation;
    assert!(apply_hook_reply(dialog, generation, update));
    let fields = dialog.fields.as_ref().unwrap();
    let detail = &fields.iter().find(|(field, _)| field.key == "detail").unwrap().0;
    assert!(!detail.visible && !detail.enabled);
    assert_eq!(detail.tip.as_deref(), Some("Header detail"));
    assert_eq!(params(fields).unwrap()["width"], 24.0);
    assert_eq!(params(fields).unwrap()["part"], "chosen");
    assert!(!dialog.has_hook, "general patches do not resubmit the file-aware schema hook");
    let dialog = h.app.plugin_menu.dialogs.last().unwrap();
    assert!(dialog.ready(&h.app).is_ok());
}

fn importing_entry() -> solidcraft_engine::plugin::Entry {
    serde_json::from_value(json!({"name":"convert","kind":"importer","effect":"open","extensions":["smp"],
        "input_schema":{"type":"object","properties":{"part":{"type":"string","default":"initial"}}}}))
    .unwrap()
}

#[test]
fn importer_menu_picks_first_and_cancel_opens_no_form_with_a_legacy_preference() {
    let mut h = crate::scenario::Harness::new();
    h.app.plugin_menu.listed = vec![listed("sample", vec![importing_entry()])];
    h.app.plugin_menu.loaded = true;
    assert!(!h.app.preferences.plugin_import_file_first, "existing workflows stay the default");
    h.app.preferences.plugin_import_file_first = true;
    h.app.services.pick_open = Some(Box::new(|extensions| {
        assert_eq!(extensions, &["smp"]);
        Box::pin(std::future::ready(None))
    }));
    open_entry(&mut h.app, 0, 0);
    assert!(h.app.plugin_menu.dialogs.is_empty(), "the picker comes before the form");
    h.frames(2);
    assert!(h.app.plugin_menu.dialogs.is_empty(), "cancel never runs the importer or opens a form");
    assert!(h.app.file_load.idle());
    h.app.preferences.plugin_import_file_first = false;
    open_entry(&mut h.app, 0, 0);
    assert!(h.app.plugin_menu.dialog().unwrap().source.is_none(), "the preference restores options first");
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app), Ok(json!({"part":"initial"})));
    let saved = serde_json::to_value(&h.app.preferences).unwrap();
    let restored: crate::prefs::Prefs = serde_json::from_value(saved).unwrap();
    assert!(!restored.plugin_import_file_first, "the legacy choice persists");
    h.app.plugin_menu.dialogs.clear();
    h.app.preferences.plugin_import_file_first = true;
    h.app.services.pick_open = Some(Box::new(|_| Box::pin(std::future::ready(Some("picked.smp".into())))));
    open_entry(&mut h.app, 0, 0);
    crate::file_picker::tick(&mut h.app, &h.ctx);
    assert_eq!(h.app.plugin_menu.dialog().unwrap().source.as_deref(), Some("picked.smp"));
    assert!(h.app.file_load.idle(), "choosing the file only starts its form");
}

#[test]
fn importer_file_generations_discard_late_updates_and_close_disconnects_the_worker() {
    let mut h = crate::scenario::Harness::new();
    h.app.preferences.plugin_import_file_first = false;
    h.app.plugin_menu.listed = vec![listed("sample", vec![importing_entry()])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.source = Some("new.smp".into());
    dialog.file_generation = 2;
    let result = |generation, name: &str| solidcraft_engine::plugin::ImportDialog {
        file: solidcraft_engine::plugin::dialog::FileFacts {
            name: name.into(),
            extension: "smp".into(),
            size: 24,
            format: "sample.convert".into(),
            generation,
        },
        schema: json!({"type":"object","properties":{"part":{"type":"string","enum":[name],"default":name}}}),
        hook: true,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    dialog.file_job = Some(FileJob { generation: 1, reply: rx });
    tx.send(Ok(result(1, "old.smp"))).unwrap();
    poll_file(dialog);
    assert!(dialog.file.is_none());
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap(), json!({"part":"initial"}));
    let (tx, rx) = std::sync::mpsc::channel();
    dialog.file_job = Some(FileJob { generation: 2, reply: rx });
    dialog.hook_values = Some(json!({"part":"new.smp"}));
    assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_err());
    tx.send(Ok(result(2, "new.smp"))).unwrap();
    poll_file(h.app.plugin_menu.dialogs.last_mut().unwrap());
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert_eq!(dialog.file.as_ref().unwrap().name, "new.smp");
    assert_eq!(dialog.ready(&h.app).unwrap(), json!({"part":"new.smp"}));
    h.app.plugin_menu.dialogs.last_mut().unwrap().hook_values = Some(json!({"part":"previous input"}));
    assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_err(), "Open cannot race the next input update");
    let (tx, rx) = std::sync::mpsc::channel();
    h.app.plugin_menu.dialogs.last_mut().unwrap().file_job = Some(FileJob { generation: 2, reply: rx });
    h.app.plugin_menu.dialogs.clear();
    assert!(tx.send(Ok(result(2, "late.smp"))).is_err(), "closing drops the result receiver");
}

#[test]
fn interactive_open_and_drop_reuse_the_file_and_do_not_start_importing_before_the_form() {
    let root = solidcraft_engine::io::vfs::temp_dir().join(format!("sc-plugin-file-form-{}", std::process::id()));
    let folder = root.join("plugins/sample");
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.2"],"runtime":{
        "module":"sample.wasm","commands":[],"entries":[importing_entry()]}});
    std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(folder.join("sample.wasm"), b"not run while choosing a file").unwrap();
    let path = root.join("part.smp").to_string_lossy().into_owned();
    std::fs::write(&path, b"header").unwrap();
    let mut h = crate::scenario::Harness::new();
    h.app.session.plugin_state.plugins_dir = Some(root.join("plugins"));
    assert!(!offer_import(&mut h.app, &path), "direct Open/drop remain the default");
    assert!(h.app.plugin_menu.dialogs.is_empty());
    h.app.preferences.plugin_import_file_first = true;
    for generation in 1..=2 {
        if generation == 1 {
            h.app.open_path_async(&path);
        } else {
            h.app.drop_path(&path);
        }
        let dialog = h.app.plugin_menu.dialog().unwrap();
        assert_eq!(dialog.source.as_deref(), Some(path.as_str()));
        assert_eq!(dialog.file_generation, generation);
        assert_eq!(h.app.plugin_menu.dialogs.len(), 1);
        assert!(h.app.file_load.idle(), "the form holds the path until Open is pressed");
        assert!(!h.app.file_picker_busy(), "Open and drops already have a file");
    }
    // Join the bounded file reader through its channel before removing the fixture's files.
    if let Some(job) = h.app.plugin_menu.dialogs.last_mut().and_then(|d| d.file_job.take()) {
        let _ = job.reply.recv();
    }
    h.app.plugin_menu.dialogs.clear();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn importer_file_facts_drive_visibility_enabled_choices_and_defaults_without_running_wasm() {
    let schema = json!({"type":"object","properties":{
        "count":{"type":"integer","default":1,"x-solidcraft-defaults":[{"when":{"file":"extension","eq":"asm"},"value":3}]},
        "part":{"type":"string","default":"whole","oneOf":[{"const":"whole"},
            {"const":"assembly","x-solidcraft-when":{"file":"extension","eq":"asm"}}]},
        "flatten":{"type":"boolean","x-solidcraft-visible-when":{"file":"format","eq":"sample.convert"},
            "x-solidcraft-enabled-when":{"file":"extension","eq":"asm"}}
    }});
    let mut file = solidcraft_engine::plugin::dialog::FileFacts {
        name: "parts.asm".into(),
        extension: "asm".into(),
        size: 42,
        format: "sample.convert".into(),
        generation: 1,
    };
    let mut fields = form(&solidcraft_engine::plugin::conditions::bind_file_conditions(&schema, Some(&file)).unwrap()).unwrap();
    assert_eq!(params(&fields).unwrap()["count"], 3);
    assert!(fields.iter().find(|(f, _)| f.key == "flatten").unwrap().0.enabled);
    set_input(&mut fields, "part", Input::Choice(Some(1)));
    set_input(&mut fields, "count", Input::Integer(8));
    file.extension = "txt".into();
    file.generation = 2;
    let mut updated = form(&solidcraft_engine::plugin::conditions::bind_file_conditions(&schema, Some(&file)).unwrap()).unwrap();
    retain_edits(&mut updated, &fields);
    refresh_conditions(&mut updated);
    assert_eq!(params(&updated).unwrap()["part"], "whole", "an unavailable edited choice resets");
    assert_eq!(params(&updated).unwrap()["count"], 8, "custom input survives the file change");
    assert!(!updated.iter().find(|(f, _)| f.key == "flatten").unwrap().0.enabled);
    file.format = "different.convert".into();
    let fields = form(&solidcraft_engine::plugin::conditions::bind_file_conditions(&schema, Some(&file)).unwrap()).unwrap();
    assert!(params(&fields).unwrap().get("flatten").is_none());
}

/// Deliver a completed hook deterministically, preserving the snapshot set when it was submitted.
fn finish_import_fields(dialog: &mut EntryDialog, schema: Value) {
    let (tx, rx) = std::sync::mpsc::channel();
    dialog.file_job = Some(FileJob { generation: dialog.file_generation, reply: rx });
    tx.send(Ok(solidcraft_engine::plugin::ImportDialog {
        file: solidcraft_engine::plugin::dialog::FileFacts {
            name: "part.smp".into(),
            extension: "smp".into(),
            size: 24,
            format: "sample.convert".into(),
            generation: dialog.file_generation,
        },
        schema,
        hook: true,
    }))
    .unwrap();
    poll_file(dialog);
}

#[test]
fn hook_refresh_preserves_cleared_text_and_incomplete_numeric_edits() {
    let schema = json!({"type":"object","properties":{
        "text":{"type":"string","default":"initial"},
        "number":{"type":"number","default":1.5},
        "integer":{"type":"integer","default":2},
        "empty_number":{"type":"number","default":3.5}
    }});
    let mut h = crate::scenario::Harness::new();
    let mut entry = importing_entry();
    entry.input_schema = schema.clone();
    h.app.plugin_menu.listed = vec![listed("sample", vec![entry])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.source = Some("part.smp".into());
    let edits = [
        ("text", Input::Text(String::new())),
        ("number", Input::Typed("-".into())),
        ("integer", Input::Typed("2e+".into())),
        ("empty_number", Input::Typed(String::new())),
    ];
    for (key, value) in &edits {
        set_input(dialog.fields.as_mut().unwrap(), key, value.clone());
    }
    let submitted = dialog_values(dialog);
    assert_eq!(submitted, json!({}), "cleared and incomplete input has no scalar value");
    dialog.hook_values = Some(submitted.clone());
    finish_import_fields(dialog, schema);
    for (key, value) in edits {
        let (field, actual) = dialog.fields.as_ref().unwrap().iter().find(|(f, _)| f.key == key).unwrap();
        assert_eq!(*actual, value, "{key} must keep the raw edit");
        assert!(field.edited);
    }
    assert_eq!(dialog_values(dialog), submitted, "hook refresh must not restore omitted defaults");
}

#[test]
fn bounded_text_choice_updates_remain_drawable_and_keep_original_constraints() {
    let schema = json!({"type":"object","properties":{
        "part":{"type":"string","minLength":4,"maxLength":32,"format":"text","default":"plate"}
    }});
    for update in [
        json!({"fields":{"part":{"enum":["plate","cover"],"default":"plate"}}}),
        json!({"fields":{"part":{"oneOf":[{"const":"plate","title":"Plate"},{"const":"cover","title":"Cover"}],"default":"plate"}}}),
    ] {
        let updated = solidcraft_engine::plugin::dialog::apply_update(&schema, &update).unwrap();
        assert_eq!(updated["properties"]["part"]["minLength"], 4);
        assert_eq!(updated["properties"]["part"]["maxLength"], 32);
        let mut h = crate::scenario::Harness::new();
        let mut entry = importing_entry();
        entry.input_schema = schema.clone();
        h.app.plugin_menu.listed = vec![listed("sample", vec![entry])];
        h.app.plugin_menu.loaded = true;
        open_entry(&mut h.app, 0, 0);
        let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
        dialog.source = Some("part.smp".into());
        dialog.hook_values = Some(dialog_values(dialog));
        finish_import_fields(dialog, updated);
        assert!(!dialog.file_failed, "valid bounded choices must not disable the form");
        assert!(matches!(dialog.fields.as_ref().unwrap()[0].0.kind, Kind::Choice { .. }));
        assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap(), json!({"part":"plate"}));
    }
    for value in [json!("bad"), json!("x".repeat(33))] {
        let update = json!({"fields":{"part":{"enum":[value]}}});
        assert!(solidcraft_engine::plugin::dialog::apply_update(&schema, &update).is_err());
    }
    assert!(
        form(&json!({"type":"object","properties":{
            "part":{"type":"string","maxLength":3,"enum":["plate"]}
        }}))
        .is_none(),
        "static choices must obey the same length constraints"
    );
}

#[test]
fn hook_defaults_trigger_another_update_until_submitted_values_match_the_form() {
    let schema = json!({"type":"object","properties":{
        "mode":{"type":"boolean","default":false,"x-solidcraft-defaults":[
            {"when":{"file":"extension","eq":"smp"},"value":true}
        ]},
        "part":{"type":"string","default":"same"}
    }});
    let mut h = crate::scenario::Harness::new();
    let mut entry = importing_entry();
    entry.input_schema = schema.clone();
    h.app.plugin_menu.listed = vec![listed("sample", vec![entry])];
    h.app.plugin_menu.loaded = true;
    open_entry(&mut h.app, 0, 0);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.source = Some("part.smp".into());
    dialog.hook_values = Some(dialog_values(dialog));
    let submitted = dialog.hook_values.clone();
    assert_eq!(submitted, Some(json!({"mode":false,"part":"same"})));
    let file = solidcraft_engine::plugin::dialog::FileFacts {
        name: "part.smp".into(),
        extension: "smp".into(),
        size: 24,
        format: "sample.convert".into(),
        generation: dialog.file_generation,
    };
    let bound = solidcraft_engine::plugin::conditions::bind_file_conditions(&schema, Some(&file)).unwrap();
    let first =
        solidcraft_engine::plugin::dialog::apply_update(&bound, &json!({"fields":{"part":{"enum":["same","old"],"default":"same"}}})).unwrap();
    finish_import_fields(dialog, first);
    assert_eq!(dialog.hook_values, submitted, "displayed defaults were never submitted to this hook");
    assert_eq!(dialog_values(dialog), json!({"mode":true,"part":"same"}));
    assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_err(), "Open waits for choices computed for mode=true");
    poll(&mut h.app);
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    assert!(dialog.file_job.is_some(), "poll queues the missing update");
    assert_eq!(dialog.hook_values, Some(json!({"mode":true,"part":"same"})));
    let second =
        solidcraft_engine::plugin::dialog::apply_update(&bound, &json!({"fields":{"part":{"enum":["same","new"],"default":"same"}}})).unwrap();
    finish_import_fields(dialog, second);
    assert_eq!(dialog.hook_values.as_ref(), Some(&dialog_values(dialog)));
    assert_eq!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).unwrap(), json!({"mode":true,"part":"same"}));
    poll(&mut h.app);
    assert!(h.app.plugin_menu.dialog().unwrap().file_job.is_none(), "unchanged values must not queue another update");
}

#[test]
fn dialog_hook_folder_defaults_do_not_grant_access_or_overwrite_a_browsed_folder() {
    use solidcraft_engine::plugin::dialog::apply;

    let mut h = hook_dialog_harness();
    let root = h.dir.join("hook-library");
    let other = h.dir.join("hook-other");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let schema = json!({"type":"object","properties":{
        "folder":{"type":"string","format":"directory","x-solidcraft-grant":"read"}
    }});
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    dialog.fields = form(&schema);
    dialog.hook.as_mut().unwrap().schema = schema.clone();
    let reply = json!({"folder":{"default":root}}).to_string();
    let update = apply(&schema, &json!({}), &Default::default(), Some(&reply), false).unwrap();
    assert!(apply_hook_reply(dialog, 0, update));
    assert_eq!(params(dialog.fields.as_ref().unwrap()).unwrap()["folder"], root.to_string_lossy().as_ref());
    assert!(dialog.folder_consent().unwrap().folders.is_empty(), "a hook default is not user consent");
    let id = dialog.id;
    apply_browsed_folder(&mut h.app, id, "folder", root.to_string_lossy().into_owned());
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let protected = std::collections::BTreeSet::from(["folder".into()]);
    let reply = json!({"folder":{"value":other}}).to_string();
    let update = apply(&schema, &hook_values(dialog.fields.as_ref().unwrap()), &protected, Some(&reply), false).unwrap();
    assert!(apply_hook_reply(dialog, 0, update));
    let consent = dialog.folder_consent().unwrap();
    assert_eq!(consent.folders.len(), 1);
    assert_eq!(consent.folders[0].folder, std::fs::canonicalize(&root).unwrap(), "the browsed value remains the granted root");
    for patch in [json!({"folder":{"visible":false}}), json!({"folder":{"enabled":false}})] {
        let reply = patch.to_string();
        let update = apply(&schema, &hook_values(dialog.fields.as_ref().unwrap()), &protected, Some(&reply), false).unwrap();
        assert!(apply_hook_reply(dialog, 0, update));
        assert!(dialog.folder_consent().unwrap().folders.is_empty(), "hook-hidden or disabled fields add no consent");
    }
}

#[test]
fn annotated_folder_fields_grant_only_edited_visible_values_for_the_internal_request() {
    let mut h = crate::scenario::Harness::new();
    let root = h.dir.join("library");
    let pasted = h.dir.join("pasted");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&pasted).unwrap();
    let mut command = entry("load", "read", json!([]));
    command.input_schema = json!({"type":"object","properties":{
        "use_library":{"type":"boolean","default":true},
        "folder":{"type":"string","title":"Library","format":"directory","default":root,
            "x-solidcraft-grant":"read","x-solidcraft-visible-when":{"field":"use_library","eq":true}}
    }});
    h.app.plugin_menu.listed = vec![listed("files", vec![command])];
    h.app.services.pick_folder = Some(Box::new(|_| Box::pin(std::future::pending())));
    open_entry(&mut h.app, 0, 0);
    h.frames(2);
    let note = h.app.tr("ui.plugin_dialog_folder_read_once").to_owned();
    assert!(!h.widgets(&note).is_empty(), "the field explains the run-only read permission");
    assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_err(), "a default is not consent");
    let id = h.app.plugin_menu.dialog().unwrap().id;
    apply_browsed_folder(&mut h.app, id, "folder", root.to_string_lossy().into_owned());
    assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_ok(), "Browse consents even when it selects the default");
    h.frames(2);
    let editor = crate::scenario::handle_at("plugin_dialog:field:folder").expect("directory text editor is drawn");
    let browse = crate::scenario::handle_at("plugin_dialog:browse:folder").expect("Browse has a separate handle");
    assert_ne!(editor, browse);
    h.step(&json!({"click":{"handle":"plugin_dialog:field:folder"}})).unwrap();
    h.step(&json!({"key":"A","ctrl":true,"cmd":true})).unwrap();
    h.step(&json!({"text":pasted.to_string_lossy()})).unwrap();
    let consent = h.app.plugin_menu.dialog().unwrap().folder_consent().unwrap();
    assert_eq!(consent.folders[0].folder, std::fs::canonicalize(&pasted).unwrap());
    run(&mut h.app).unwrap();
    let request = h.app.control_held.back().unwrap();
    let serial = request.serial;
    assert_eq!(request.params["params"]["params"]["folder"], pasted.to_string_lossy().as_ref());
    assert!(request.params["params"].get("dialog_folders").is_none(), "consent is not a JSON parameter");
    assert!(take_folder_consent(&mut h.app, serial + 100).is_none());
    assert_eq!(take_folder_consent(&mut h.app, serial), Some(consent));
    assert!(take_folder_consent(&mut h.app, serial).is_none(), "one request consumes it");
    assert!(h.app.session.plugin_state.grants.is_empty() && h.app.preferences.plugin_grants.is_empty());
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    let fields = dialog.fields.as_mut().unwrap();
    fields.iter_mut().find(|(f, _)| f.key == "use_library").unwrap().1 = Input::Boolean(false);
    refresh_conditions(fields);
    assert!(dialog.folder_consent().unwrap().folders.is_empty(), "a hidden value adds no grant");
}

#[test]
fn stored_folder_values_need_actual_consent_and_file_refresh_keeps_a_chosen_folder() {
    let mut h = crate::scenario::Harness::new();
    let root = h.dir.join("stored-library");
    std::fs::create_dir_all(&root).unwrap();
    let schema = json!({"type":"object","properties":{
        "library":{"type":"string","format":"directory","x-solidcraft-grant":"read"}
    }});
    let mut command = entry("load", "read", json!([]));
    command.input_schema = schema.clone();
    h.app.plugin_menu.listed = vec![listed("files", vec![command])];
    h.app.plugin_menu.loaded = true;
    open_form(&mut h.app, "files", "load", &json!({"library":root})).unwrap();
    checked_prefill(&h.app);
    let dialog = h.app.plugin_menu.dialog().unwrap();
    assert!(dialog.fields.as_ref().unwrap()[0].0.edited, "stored input remains protected from hook defaults");
    assert!(dialog.folder_consent().unwrap().folders.is_empty(), "stored values do not grant read access");
    assert!(dialog.ready(&h.app).is_err());
    let id = dialog.id;
    apply_browsed_folder(&mut h.app, id, "library", root.to_string_lossy().into_owned());
    assert!(h.app.plugin_menu.dialog().unwrap().ready(&h.app).is_ok(), "choosing the same path supplies consent");
    h.app.plugin_menu.dialogs.clear();
    let mut importer = importing_entry();
    importer.input_schema = schema.clone();
    h.app.plugin_menu.listed = vec![listed("files", vec![importer])];
    open_entry(&mut h.app, 0, 0);
    let id = h.app.plugin_menu.dialog().unwrap().id;
    apply_browsed_folder(&mut h.app, id, "library", root.to_string_lossy().into_owned());
    let before = h.app.plugin_menu.dialog().unwrap().folder_consent().unwrap();
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    // A completed file-first form refresh rebuilds fields, retaining real user interaction.
    finish_import_fields(dialog, schema);
    assert_eq!(dialog.folder_consent().unwrap(), before);
}

#[test]
fn scoped_ui_file_import_keeps_its_caller_scope_when_submitted_from_a_normal_frame() {
    let mut h = crate::scenario::Harness::new();
    let root = h.dir.join("allowed");
    let folder = root.join("plugins/sample");
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.2"],"runtime":{
        "module":"sample.wasm","commands":[],"entries":[importing_entry()]}});
    std::fs::write(folder.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(folder.join("sample.wasm"), b"not executed by the dialog").unwrap();
    let path = root.join("part.smp").to_string_lossy().into_owned();
    std::fs::write(&path, b"header").unwrap();
    h.app.session.plugin_state.plugins_dir = Some(root.join("plugins"));
    h.app.preferences.plugin_import_file_first = true;
    let reply = h.call("ui.file", json!({"path":path,"allow_paths":[root]}));
    assert_eq!(reply["ok"], true, "{reply}");
    let id = h.app.plugin_menu.dialog().unwrap().id;
    // Join the facts worker without using frame timing; submit outside the caller's scope.
    let dialog = h.app.plugin_menu.dialogs.last_mut().unwrap();
    if let Some(job) = dialog.file_job.take() {
        let update = job.reply.recv().unwrap().unwrap();
        dialog.file = Some(update.file);
        dialog.fields = form(&update.schema);
        dialog.hook_values = Some(dialog_values(dialog));
    }
    assert!(solidcraft_engine::io::vfs::current_scope().is_none());
    h.app.services.pick_open = Some(Box::new(|_| Box::pin(std::future::pending())));
    h.app.pick_open_for(&["smp"], |_, _| {});
    run_dialog(&mut h.app, id).unwrap();
    let request = h.app.file_load.queued_requests().back().unwrap();
    assert_eq!(request.kind, crate::file_load::Kind::Open);
    assert_eq!(request.path, path);
    let scope = request.scope.as_ref().expect("the scoped ui.file authority survives the form");
    assert!(scope.check(&path).is_ok());
    let outside = h.dir.join("outside-caller-scope");
    std::fs::write(&outside, b"private").unwrap();
    assert!(scope.check(&outside.to_string_lossy()).is_err());
    h.app.file_load.cancel();
}
