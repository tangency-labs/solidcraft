//! Transient Render view requests and explicit refusals for planned interactions.
use super::CommandSpec;
use crate::schema::render_lights::{MuteParams, SoloParams};
use crate::schema::{
    TypedContract, output_schema_for,
    render_ui::{BatchParams, BatchResult, CheckReport, CompareDivider, CompareParams, CompareSide, ResultIndex},
};
use crate::{Result, Session, params::bad};
use serde::Deserialize;
use serde_json::{Value, json};

const SOLO: crate::schema::TypedContract<SoloParams> = crate::schema::TypedContract::new();
const MUTE: crate::schema::TypedContract<MuteParams> = crate::schema::TypedContract::new();

fn solo(_: &mut crate::Session, params: &serde_json::Value) -> crate::Result<serde_json::Value> {
    let command = "render.lights.solo";
    if !params.is_object() {
        return Err(crate::params::bad(command, "expected an object"));
    }
    let input: SoloParams = serde_json::from_value(params.clone()).map_err(|e| crate::params::bad(command, e.to_string()))?;
    if input.solo && input.id.is_none() {
        return Err(crate::params::bad(command, "solo requires a light id"));
    }
    if let Some(id) = &input.id {
        crate::render_lights::parse_id(id).map_err(|e| crate::params::bad(command, e))?;
    }
    serde_json::to_value(input).map_err(|e| crate::params::bad(command, e.to_string()))
}

fn mute(_: &mut crate::Session, params: &serde_json::Value) -> crate::Result<serde_json::Value> {
    let command = "render.lights.mute";
    if !params.is_object() {
        return Err(crate::params::bad(command, "expected an object"));
    }
    let input: MuteParams = serde_json::from_value(params.clone()).map_err(|e| crate::params::bad(command, e.to_string()))?;
    crate::render_lights::parse_id(&input.id).map_err(|e| crate::params::bad(command, e))?;
    serde_json::to_value(input).map_err(|e| crate::params::bad(command, e.to_string()))
}
const RESULT_INDEX: TypedContract<ResultIndex> = TypedContract::new();
const COMPARE: TypedContract<CompareParams> = TypedContract::new();
const DIVIDER: TypedContract<CompareDivider> = TypedContract::new();
const BATCH: TypedContract<BatchParams> = TypedContract::new();

const CHECK_REPORT: TypedContract<CheckReport> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("render.preflight.copy", "Copy Render Check Report", crate::checked!("render.preflight.copy", copy_check))
        .icon("copy")
        .contract(&CHECK_REPORT)
        .output(output_schema_for::<CheckReport>)
        .noundo()
        .doc_only()
        .params("text: nonempty report, at most 65536 bytes. Validated clipboard request; headless echo only, never renders or edits a design."),
    CommandSpec::new("render.result.select", "Select Render Result", crate::checked!("render.result.select", select_result))
        .icon("render_image").noundo().doc_only()
        .enabled(results_ready)
        .contract(&RESULT_INDEX).output(output_schema_for::<ResultIndex>)
        .params("index: integer 0..7, oldest first; must be retained. Selects a completed result as transient per-document view state; echoes index."),
    CommandSpec::new("render.result.discard", "Discard Render Result", crate::checked!("render.result.discard", discard_result))
        .icon("delete").noundo().doc_only()
        .enabled(results_ready)
        .contract(&RESULT_INDEX).output(output_schema_for::<ResultIndex>)
        .params("index: integer 0..7, oldest first; must be retained. Drops only that transient result, selecting the newest remaining result if necessary; echoes index."),
    CommandSpec::new("render.output.size", "Set Render Output Size", output_size)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("width, height: 1..16384; custom?: bool. Validated Render output draft request; headless echo only, no design edits or job."),
    CommandSpec::new("render.result.view", "View Render Image", result_view)
        .icon("render_image")
        .noundo()
        .doc_only()
        .params("view: model|image|fit|actual. Validated result canvas request; headless echo only, never starts a job."),
    CommandSpec::new("render.result.close", "Close Render Image", close_result)
        .icon("close")
        .key("Escape")
        .noundo()
        .doc_only()
        .params("No parameters. Return to Model Preview; headless echo only, never cancels a job."),
    CommandSpec::new("appearance.paint", "Paint Appearance", super::inspect::appearance_gestures::paint)
        .icon("appearance").key("B").noundo()
        .params("appearance or color/name/opacity/pbr: appearance.assign look parameters; validates and returns look for ui.start paint. done?: true ends the UI tool. Programmatic calls never open UI."),
    CommandSpec::new("appearance.pick_up", "Pick Up Appearance", super::inspect::appearance_gestures::pick_up)
        .icon("select").key("I").noundo()
        .params("target?: appearance.chain target; returns its resolved look, or requests the UI sampling tool when omitted. Programmatic calls never open UI."),

    CommandSpec::new("render.lights.solo","Solo Light",crate::checked!("render.lights.solo",solo))
        .noundo().doc_only().icon("render_scene").at("RENDER SCENE","LIGHTS")
        .contract(&SOLO).output(crate::schema::output_schema_for::<SoloParams>)
        .params("id?: kind:slot light address; solo:bool. Enabling requires an id; false clears solo. Validates and echoes per-document UI view state. Never saves settings, evaluates geometry, opens an editor or changes history."),
    CommandSpec::new("render.lights.mute","Mute Light",crate::checked!("render.lights.mute",mute))
        .noundo().doc_only().icon("render_scene").at("RENDER SCENE","LIGHTS")
        .contract(&MUTE).output(crate::schema::output_schema_for::<MuteParams>)
        .params("id: kind:slot light address; mute:bool. Validates and echoes per-document UI view state, applied to live preview and captured render copies only. Never changes saved settings or history."),
    CommandSpec::new("render.compare", "Compare Images", crate::checked!("render.compare", compare))
        .icon("compare").noundo().doc_only().at("RENDER", "RESULT")
        .enabled(results_ready)
        .contract(&COMPARE).output(output_schema_for::<CompareParams>)
        .params("left, right: live or retained result index 0..7 (oldest first), distinct and retained; OR off:true. Validated per-document comparison view request; headless echo, no rendering or design/history change."),
    CommandSpec::new("render.compare.divider", "Move Render Comparison Divider", crate::checked!("render.compare.divider", divider))
        .icon("compare").noundo().doc_only()
        .contract(&DIVIDER).output(output_schema_for::<CompareDivider>)
        .params("position: finite number, clamped to 0.05..0.95. Transient comparison divider request; headless echo only."),
    CommandSpec::new("render.batch", "Render All Shots", crate::checked!("render.batch", batch))
        .icon("render_image").noundo().doc_only().at("RENDER", "BATCH")
        .enabled(|s| s.render_batch_ready())
        .contract(&BATCH).output(output_schema_for::<BatchResult>)
        .params("request?: render.start parameters, shared across all saved shots except camera. Default: first saved setup in name order. Captures up to 32 shot cameras and queues sequentially through render.start; returns {queued:n}. Refuses active renders, no shots or stale geometry. render.status (also with {}) and the UI tick advance the queue; Cancel on the active job drops waiting shots. No design/history/revision change."),
];

fn copy_check(_: &mut Session, params: &Value) -> Result<Value> {
    let report: CheckReport = serde_json::from_value(params.clone()).map_err(|e| bad("render.preflight.copy", e.to_string()))?;
    if report.text.is_empty() || report.text.len() > 65536 {
        return Err(bad("render.preflight.copy", "report must contain 1..65536 bytes"));
    }
    Ok(json!({"text":report.text}))
}

fn compare(s: &mut Session, params: &Value) -> Result<Value> {
    let input: CompareParams = serde_json::from_value(params.clone()).map_err(|e| bad("render.compare", e.to_string()))?;
    match input {
        CompareParams::Off(off) if off.off => {
            s.render_jobs.set_comparison(None);
            Ok(json!({"off":true}))
        }
        CompareParams::Pair(pair) => {
            if pair.left == pair.right {
                return Err(bad("render.compare", "comparison sides must differ"));
            }
            let results = s.render_retained_results()?;
            for side in [pair.left, pair.right] {
                if let CompareSide::Result(index) = side
                    && (index > 7 || results.get(index as usize).is_none())
                {
                    return Err(bad("render.compare", "retained result index is out of bounds"));
                }
            }
            let resolve = |side| match side {
                CompareSide::Live(_) => None,
                CompareSide::Result(index) => results.get(index as usize).map(|result| result.frame.job),
            };
            s.render_jobs.set_comparison(Some([resolve(pair.left), resolve(pair.right)]));
            Ok(json!({"left":pair.left,"right":pair.right}))
        }
        _ => Err(bad("render.compare", "off must be true")),
    }
}

fn divider(_: &mut Session, params: &Value) -> Result<Value> {
    let input: CompareDivider = serde_json::from_value(params.clone()).map_err(|e| bad("render.compare.divider", e.to_string()))?;
    if !input.position.is_finite() {
        return Err(bad("render.compare.divider", "position must be finite"));
    }
    Ok(json!({"position":input.position.clamp(0.05,0.95)}))
}

fn batch(s: &mut Session, params: &Value) -> Result<Value> {
    let input: BatchParams = serde_json::from_value(params.clone()).map_err(|e| bad("render.batch", e.to_string()))?;
    s.render_batch_ready().map_err(|reason| bad("render.batch", reason))?;
    if s.model.is_stale() {
        return Err(bad("render.batch", "finish the modelling gesture before rendering shots"));
    }
    for name in s.doc.render.setups.keys() {
        solidcraft_doc::render_settings::RenderSettings::validate_name(name).map_err(|error| bad("render.batch", error.to_string()))?;
    }
    let common = if let Some(request) = input.request {
        Value::Object(request.into_iter().collect())
    } else {
        let setup = s.doc.render.setups.values().next().ok_or_else(|| bad("render.batch", "no saved shots"))?;
        json!({"settings":setup})
    };
    let mut request = common.as_object().cloned().ok_or_else(|| bad("render.batch", "request must be an object"))?;
    let settings = if let Some(name) = request.get("setup") {
        let name = name.as_str().ok_or_else(|| bad("render.batch", "setup must be a saved name"))?;
        if request.contains_key("settings") {
            return Err(bad("render.batch", "choose either setup or settings"));
        }
        s.doc.render.setups.get(name).cloned().ok_or_else(|| bad("render.batch", "unknown saved setup"))?
    } else if let Some(settings) = request.get("settings") {
        serde_json::from_value(settings.clone()).map_err(|e| bad("render.batch", e.to_string()))?
    } else {
        s.doc.render.setups.values().next().cloned().ok_or_else(|| bad("render.batch", "no saved shots"))?
    };
    request.remove("setup");
    // A batch never replaces an active job; malformed replace values must still refuse.
    if request.get("replace").is_some_and(|value| value != &json!(false)) {
        return Err(bad("render.batch", "replace must be false or omitted"));
    }
    request.insert("settings".into(), json!(settings));
    let request = std::sync::Arc::new(Value::Object(request));
    let shots = s
        .doc
        .render
        .setups
        .iter()
        .map(|(name, shot)| crate::render_jobs::BatchShot { name: name.clone(), camera: shot.camera, request: request.clone() })
        .collect();
    let queued = s.start_render_batch(shots)?;
    Ok(json!({"queued":queued}))
}

fn results_ready(s: &Session) -> std::result::Result<(), String> {
    if s.render_jobs.has_results() { Ok(()) } else { Err("no completed render results are retained".into()) }
}

fn indexed_result(s: &mut Session, params: &Value, command: &str, discard: bool) -> Result<Value> {
    let input: ResultIndex = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.index > 7 {
        return Err(bad(command, "index must be in 0..7"));
    }
    s.render_jobs.result_index(input.index as usize, discard).map_err(crate::render_jobs::job_error)?;
    Ok(json!({"index":input.index}))
}

fn select_result(s: &mut Session, params: &Value) -> Result<Value> {
    indexed_result(s, params, "render.result.select", false)
}

fn discard_result(s: &mut Session, params: &Value) -> Result<Value> {
    indexed_result(s, params, "render.result.discard", true)
}

#[cfg(test)]
#[test]
fn render_lights_remaining_planned_commands_refuse_without_mutation() {
    use serde_json::{Value, json};
    assert!(super::find_command("appearance.replace").is_some_and(|command| command.planned.is_none()));
    let mut session = crate::Session::default();
    session.execute("render.settings.set", &json!({"name":"Studio","set":{"width":800}})).unwrap();
    session.execute("render.settings.set", &json!({"name":"Studio","set":{"width":900}})).unwrap();
    session.execute("edit.undo", &json!({})).unwrap();
    let document = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty());
    for command in COMMANDS {
        if command.planned.is_none() {
            continue;
        }
        assert!(!command.info(&session).enabled);
        for params in [json!({}), Value::Null, json!([]), json!({"target":1e300,"name":"hostile"})] {
            let error = session.execute(command.id, &params).unwrap_err().to_string();
            assert!(error.contains("not supported yet"), "{error}");
            assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
            assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty()));
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputSize {
    width: u32,
    height: u32,
    #[serde(default)]
    custom: bool,
}
fn output_size(_: &mut Session, params: &Value) -> Result<Value> {
    let input: OutputSize = serde_json::from_value(params.clone()).map_err(|e| bad("render.output.size", e.to_string()))?;
    if !(1..=16384).contains(&input.width) || !(1..=16384).contains(&input.height) {
        return Err(bad("render.output.size", "dimensions must be in 1..16384"));
    }
    let mut reply = json!({"width":input.width,"height":input.height});
    if input.custom {
        reply["custom"] = json!(true);
    }
    Ok(reply)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultView {
    view: String,
}
fn result_view(s: &mut Session, params: &Value) -> Result<Value> {
    let input: ResultView = serde_json::from_value(params.clone()).map_err(|e| bad("render.result.view", e.to_string()))?;
    if !matches!(input.view.as_str(), "model" | "image" | "fit" | "actual") {
        return Err(bad("render.result.view", "view must be model, image, fit or actual"));
    }
    if input.view == "model" {
        s.render_jobs.set_comparison(None);
    }
    Ok(json!({"view":input.view}))
}
fn close_result(s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("render.result.close", "expected no parameters"));
    }
    s.render_jobs.set_comparison(None);
    Ok(json!({"view":"model"}))
}

#[cfg(test)]
#[test]
fn output_viewer_requests_are_validated_headless_echoes() {
    let mut s = Session::default();
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.evaluations);
    for size in [json!({"width":1280,"height":720}), json!({"width":1920,"height":1080})] {
        assert_eq!(s.execute("render.output.size", &size).unwrap(), size);
    }
    let custom = json!({"width":1920,"height":1080,"custom":true});
    assert_eq!(s.execute("render.output.size", &custom).unwrap(), custom);
    assert!(s.execute("render.output.size", &json!({"width":1920,"height":1080,"custom":"yes"})).is_err());
    for view in ["model", "image", "fit", "actual"] {
        let request = json!({"view":view});
        assert_eq!(s.execute("render.result.view", &request).unwrap(), request);
    }
    assert_eq!(s.execute("render.result.close", &json!({})).unwrap(), json!({"view":"model"}));
    for request in
        [json!(null), json!({}), json!({"width":0,"height":1}), json!({"width":u64::MAX,"height":1}), json!({"width":1,"height":1,"extra":true})]
    {
        assert!(s.execute("render.output.size", &request).is_err());
    }
    for request in [json!(null), json!({"view":"unknown"}), json!({"view":"image","extra":true})] {
        assert!(s.execute("render.result.view", &request).is_err());
        // Session normalizes null parameters to an empty object for no-argument commands.
        if request.is_object() {
            assert!(s.execute("render.result.close", &request).is_err());
        }
    }
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.evaluations));
}

#[cfg(test)]
#[test]
fn output_sections_copy_report_validates_without_design_history_or_job_changes() {
    let mut s = Session::default();
    let doc = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty());
    let report = json!({"text":"Check\n✓ Size is within the renderer limit"});
    assert_eq!(s.execute("render.preflight.copy", &report).unwrap(), report);
    for request in [json!({}), json!({"text":""}), json!({"text":true}), json!({"text":"check","job":1}), json!({"text":"x".repeat(65537)})] {
        assert!(s.execute("render.preflight.copy", &request).is_err());
    }
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty()));
}

#[cfg(test)]
fn complete_result(s: &mut Session, shot: &str) -> solidcraft_render::job::JobId {
    let reply = s.execute("render.start", &json!({"setup":shot})).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    let host = s.render_job_host().unwrap().clone();
    for _ in 0..1000 {
        host.pump(4096).unwrap();
        if s.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"] == "completed" {
            return job;
        }
    }
    panic!("bounded result fixture did not complete");
}

#[cfg(test)]
#[test]
fn retained_results_cap_order_metadata_and_selection_never_edit_a_design() {
    let mut s = Session::default();
    s.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    s.execute("render.settings.set", &json!({"name":"Hero","set":{"width":16,"height":8}})).unwrap();
    s.execute("render.settings.set", &json!({"name":"Hero","set":{"width":32}})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.redo.len(), 1, "view commands must preserve an available redo step");
    let document = s.doc.clone();
    let before = (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty());
    let jobs: Vec<_> = (0..10).map(|_| complete_result(&mut s, "Hero")).collect();
    let results = s.render_retained_results().unwrap();
    assert_eq!(results.len(), 8);
    assert_eq!(results.iter().map(|result| result.frame.job).collect::<Vec<_>>(), jobs[2..]);
    for result in &results {
        assert_eq!(result.shot.as_deref(), Some("Hero"));
        assert_eq!((result.frame.width, result.frame.height), (16, 8));
        assert!(result.seconds.is_finite() && result.seconds >= 0.0);
        assert_eq!(result.frame.scene_generation, s.revision);
        assert!(!result.stale);
    }
    assert!(s.render_job_handle(jobs[0]).is_err());
    assert!(s.render_job_handle(jobs[1]).is_err());
    assert_eq!(s.render_selected_result(), Some(jobs[9]));
    assert_eq!(s.execute("render.result.select", &json!({"index":0})).unwrap(), json!({"index":0}));
    assert_eq!(s.render_selected_result(), Some(jobs[2]));
    assert_eq!(s.execute("render.result.copy", &json!({})).unwrap()["frame"]["job"], json!(jobs[2]));
    assert_eq!(s.execute("render.result.pin", &json!({})).unwrap()["frame"]["job"], json!(jobs[2]));
    s.execute("render.result.discard", &json!({"index":0})).unwrap();
    assert_eq!(s.render_selected_result(), Some(jobs[9]));
    assert!(s.render_job_handle(jobs[2]).is_err());
    s.execute("render.result.select", &json!({"index":2})).unwrap();
    s.execute("render.result.discard", &json!({"index":0})).unwrap();
    assert_eq!(s.render_selected_result(), Some(jobs[5]), "discarding an earlier row preserves the selected identity");
    while !s.render_retained_results().unwrap().is_empty() {
        s.execute("render.result.discard", &json!({"index":0})).unwrap();
    }
    assert!(s.render_selected_result().is_none());
    assert!(s.execute("render.result.copy", &json!({})).is_err());
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty()));
}

#[cfg(test)]
#[test]
fn retained_result_requests_refuse_hostile_and_missing_indices_atomically() {
    let mut s = Session::default();
    s.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    s.execute("render.settings.set", &json!({"name":"Front","set":{"width":8,"height":8}})).unwrap();
    let job = complete_result(&mut s, "Front");
    let document = s.doc.clone();
    let before = (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty());
    let retained = s.render_job_host().unwrap().admitted_bytes();
    for mode in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Strict] {
        s.param_check = mode;
        for command in ["render.result.select", "render.result.discard"] {
            let spec = super::find_command(command).unwrap();
            assert!(spec.contract.is_some() && spec.output.is_some());
            for request in [
                json!(null),
                json!([]),
                json!({}),
                json!({"index":-1}),
                json!({"index":0.5}),
                json!({"index":1e300}),
                json!({"index":u64::MAX}),
                json!({"index":"0"}),
                json!({"index":null}),
                json!({"index":true}),
                json!({"index":1}),
                json!({"index":8}),
                json!({"index":0,"extra":true}),
            ] {
                assert!(s.execute(command, &request).is_err(), "{command}: {request}");
                assert_eq!(s.render_retained_results().unwrap().len(), 1);
                assert_eq!(s.render_selected_result(), Some(job));
                assert_eq!(s.render_job_host().unwrap().admitted_bytes(), retained);
                assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
                assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty()));
            }
        }
    }
    s.execute("render.settings.set", &json!({"name":"Front","set":{"width":16}})).unwrap();
    assert!(s.render_retained_results().unwrap()[0].stale);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.render_retained_results().unwrap()[0].stale, "undo does not recapture pixels");
    assert_eq!(s.render_retained_results().unwrap()[0].frame.scene_generation, before.2);
    let mut other = Session::default();
    assert!(other.render_retained_results().unwrap().is_empty());
    assert!(other.execute("render.result.select", &json!({"index":0})).is_err());
    assert!(other.render_job_handle(job).is_err());
}

#[cfg(test)]
#[test]
fn render_results_compare_validates_both_sides_and_clamps_only_finite_dividers_without_mutation() {
    let mut s = Session::default();
    s.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    s.execute("render.settings.set", &json!({"name":"Hero","set":{"width":8,"height":8}})).unwrap();
    complete_result(&mut s, "Hero");
    complete_result(&mut s, "Hero");
    s.execute("render.settings.set", &json!({"name":"Hero","set":{"width":16}})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty());
    let selected = s.selection.clone();
    for mode in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Strict] {
        s.param_check = mode;
        for request in [json!({"left":"live","right":0}), json!({"left":1,"right":0}), json!({"left":0,"right":"live"}), json!({"off":true})] {
            assert_eq!(s.execute("render.compare", &request).unwrap(), request);
        }
        for request in [
            json!({}),
            json!([]),
            json!({"off":false}),
            json!({"off":true,"left":0}),
            json!({"left":"live","right":"live"}),
            json!({"left":0,"right":0}),
            json!({"left":"unknown","right":0}),
            json!({"left":"live","right":2}),
            json!({"left":8,"right":0}),
            json!({"left":-1,"right":0}),
            json!({"left":0.5,"right":0}),
            json!({"left":1e300,"right":"live"}),
            json!({"left":null,"right":0}),
            json!({"left":"0","right":1}),
            json!({"left":true,"right":0}),
            json!({"left":"live","right":0,"extra":true}),
        ] {
            assert!(s.execute("render.compare", &request).is_err(), "{request}");
        }
        for (position, expected) in [(-1e300, 0.05), (0.0, 0.05), (0.25, 0.25), (1.0, 0.95), (1e300, 0.95)] {
            assert_eq!(s.execute("render.compare.divider", &json!({"position":position})).unwrap(), json!({"position":expected}));
        }
        for request in [json!({}), json!({"position":null}), json!({"position":"NaN"}), json!({"position":true}), json!({"position":0.5,"extra":1})] {
            assert!(s.execute("render.compare.divider", &request).is_err());
        }
        assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
        assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty()));
        assert_eq!(s.selection, selected);
        assert_eq!(s.render_retained_results().unwrap().len(), 2);
    }
    let mut empty = Session::default();
    assert!(empty.execute("render.compare", &json!({"left":"live","right":0})).is_err());
    assert!(empty.execute("render.compare", &json!({"off":true})).is_err());
    for id in ["render.compare", "render.compare.divider", "render.batch"] {
        let command = super::find_command(id).unwrap();
        assert!(command.contract.is_some() && command.output.is_some() && command.planned.is_none());
        assert!(!command.undoable && !command.reads_model);
    }
}

#[cfg(test)]
fn batch_fixture(count: usize) -> Session {
    let mut s = Session::default();
    s.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    for index in 0..count {
        let camera = solidcraft_render::Camera { yaw: index as f64 * 0.2, roll: index as f64 * 0.01, ..Default::default() };
        s.execute("render.settings.set", &json!({"name":format!("Shot {index:02}"),"set":{"camera":camera,"width":8+index,"height":8}})).unwrap();
    }
    s
}

#[cfg(test)]
#[test]
fn render_results_batch_is_sequential_uses_snapshot_cameras_and_one_setup_without_design_or_history_changes() {
    let mut s = batch_fixture(3);
    s.execute("render.settings.set", &json!({"name":"Shot 00","set":{"width":64}})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty());
    let cameras: Vec<_> = s.doc.render.setups.values().map(|setup| setup.camera).collect();
    s.param_check = crate::schema::ParamCheck::Strict;
    assert_eq!(
        s.execute("render.batch", &json!({"request":{"settings":{"width":12,"height":10,"background":[7,8,9]}}})).unwrap(),
        json!({"queued":3})
    );
    assert_eq!((s.render_queue_state().running, s.render_queue_state().waiting), (1, 2));
    assert!(s.execute("render.batch", &json!({})).is_err());
    let host = s.render_job_host().unwrap().clone();
    let mut jobs = Vec::new();
    for (index, camera) in cameras.iter().enumerate() {
        let queue = s.render_queue_state();
        assert_eq!((queue.running, queue.waiting), (1, 2 - index));
        let job = queue.job.unwrap();
        let status = s.render_job_handle(job).unwrap().status().unwrap();
        assert_eq!(status.request.camera, *camera);
        assert_eq!((status.request.width, status.request.height, status.request.background), (12, 10, [7, 8, 9]));
        assert_eq!(queue.shot, Some(format!("Shot {index:02}")));
        jobs.push(job);
        for _ in 0..100 {
            if !host.pump(4096).unwrap() {
                break;
            }
        }
        let reply = s.execute("render.status", &json!({"job":job})).unwrap();
        assert_eq!(reply["progress"]["phase"], "completed");
    }
    assert_eq!((s.render_queue_state().running, s.render_queue_state().waiting), (0, 0));
    assert!(s.render_queue_state().job.is_none());
    let results = s.render_retained_results().unwrap();
    assert_eq!(results.iter().map(|result| result.frame.job).collect::<Vec<_>>(), jobs);
    assert_eq!(results.iter().map(|result| result.shot.clone().unwrap()).collect::<Vec<_>>(), ["Shot 00", "Shot 01", "Shot 02"]);
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty()));
}

#[cfg(test)]
#[test]
fn render_results_batch_caps_at_32_and_cancel_discards_every_waiting_shot() {
    let mut s = batch_fixture(32);
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty());
    assert_eq!(s.execute("render.batch", &json!({})).unwrap(), json!({"queued":32}));
    let queue = s.render_queue_state();
    assert_eq!((queue.running, queue.waiting), (1, 31));
    let job = queue.job.unwrap();
    assert!(s.execute("render.start", &json!({"settings":{"width":8,"height":8}})).is_err());
    s.execute("render.cancel", &json!({})).unwrap();
    assert_eq!(s.render_queue_state().waiting, 0);
    assert!(s.render_queue_state().job.is_none());
    s.render_job_host().unwrap().pump(4096).unwrap();
    assert!(s.render_job_handle(job).unwrap().frame().unwrap().is_none());
    for _ in 0..3 {
        assert_eq!(s.execute("render.status", &json!({})).unwrap()["queue"]["waiting"], 0);
    }
    assert!(s.render_retained_results().unwrap().is_empty());
    let next = s.execute("render.start", &json!({"settings":{"width":8,"height":8}})).unwrap();
    assert_eq!(next["job"]["serial"], 2, "Cancel must not start any waiting shot");
    s.execute("render.cancel", &json!({"job":next["job"]})).unwrap();
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty()));
    std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut s.doc).render).setups.insert("Over limit".into(), Default::default());
    assert!(s.execute("render.batch", &json!({})).unwrap_err().to_string().contains("32"));
    assert_eq!(s.render_queue_state().waiting, 0);
}

#[cfg(test)]
#[test]
fn render_results_batch_refuses_missing_shots_busy_hostile_and_failed_requests_without_queuing() {
    let mut empty = batch_fixture(0);
    assert!(empty.execute("render.batch", &json!({})).is_err());
    let mut s = batch_fixture(2);
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty());
    for request in [
        json!([]),
        json!({"request":1e300}),
        json!({"request":[]}),
        json!({"request":{"bogus":true}}),
        json!({"request":{"replace":true}}),
        json!({"request":{"settings":{"width":0}}}),
        json!({"extra":true}),
    ] {
        assert!(s.execute("render.batch", &request).is_err(), "{request}");
        assert_eq!((s.render_queue_state().running, s.render_queue_state().waiting), (0, 0));
        assert!(s.render_queue_state().job.is_none());
    }
    let job = s.execute("render.start", &json!({"settings":{"width":8,"height":8}})).unwrap()["job"].clone();
    assert!(s.execute("render.batch", &json!({})).is_err());
    assert_eq!((s.render_queue_state().running, s.render_queue_state().waiting), (1, 0));
    s.execute("render.cancel", &json!({"job":job})).unwrap();
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty()));
    s.set_render_job_host(None).unwrap();
    assert!(s.execute("render.batch", &json!({})).is_err());
    assert_eq!((s.render_queue_state().running, s.render_queue_state().waiting), (0, 0));
    assert!(s.render_queue_state().error.is_some());
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations, s.is_dirty()));
}

#[cfg(test)]
#[test]
fn render_results_batch_cameras_and_common_setup_are_captured_before_later_shot_edits() {
    let mut s = batch_fixture(2);
    let original = s.doc.render.setups.get("Shot 01").unwrap().camera;
    s.execute("render.batch", &json!({"request":{"setup":"Shot 00"}})).unwrap();
    let first = s.render_queue_state().job.unwrap();
    let changed = solidcraft_render::Camera { yaw: 2.0, ..Default::default() };
    s.execute("render.settings.set", &json!({"name":"Shot 01","set":{"camera":changed}})).unwrap();
    s.execute("render.settings.set", &json!({"name":"Shot 00","set":{"width":64}})).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations);
    let host = s.render_job_host().unwrap().clone();
    for _ in 0..100 {
        if !host.pump(4096).unwrap() {
            break;
        }
    }
    let reply = s.execute("render.status", &json!({})).unwrap();
    let next: solidcraft_render::job::JobId = serde_json::from_value(reply["queue"]["job"].clone()).unwrap();
    assert_ne!(next, first);
    let request = s.render_job_handle(next).unwrap().status().unwrap().request;
    assert_eq!(request.camera, original);
    assert_eq!(request.width, 8);
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations));
    s.execute("render.cancel", &json!({"job":next})).unwrap();
}

#[cfg(test)]
#[test]
fn render_results_compare_keeps_copy_on_the_visible_result_until_a_source_is_evicted() {
    let mut s = batch_fixture(1);
    let first = complete_result(&mut s, "Shot 00");
    complete_result(&mut s, "Shot 00");
    s.execute("render.compare", &json!({"left":"live","right":0})).unwrap();
    assert_eq!(s.render_comparison(), Some([None, Some(first)]));
    assert_eq!(s.render_selected_result(), Some(first));
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations);
    let mut newest = first;
    for index in 0..7 {
        newest = complete_result(&mut s, "Shot 00");
        if index < 6 {
            assert_eq!(s.execute("render.result.copy", &json!({})).unwrap()["frame"]["job"], json!(first));
            assert_eq!(s.render_comparison(), Some([None, Some(first)]));
        }
    }
    assert!(s.render_comparison().is_none());
    assert_eq!(s.render_selected_result(), Some(newest));
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations));
}
