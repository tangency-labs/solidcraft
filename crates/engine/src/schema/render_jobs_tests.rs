use serde_json::{Value, json};
use solidcraft_render::job::{CpuHost, JobId, Phase};

use super::{
    CancelParams, CopyParams, ExportParams, ImageCopied, ImageExported, JobPhase, PinParams, RegradeParams, RenderCancelled, RenderProgress,
    ResultPinned, StatusParams,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A job no session has: the render identities count up from 1.
fn foreign_job() -> Value {
    json!({"session": u64::MAX, "serial": 1, "generation": 1})
}

/// Pump a cooperative host until its jobs are done, as `cmd/render/job_tests.rs` does.
fn finish(host: &CpuHost) {
    for _ in 0..1000 {
        if !host.pump(4096).unwrap_or_else(|e| panic!("{e}")) {
            return;
        }
    }
    panic!("cooperative job did not finish");
}

/// Render Image on `s`, 16 × 16, returning the job.
fn start(s: &mut Session) -> JobId {
    let started = s.execute("render.start", &json!({"settings": {"width": 16, "height": 16}})).unwrap_or_else(|e| panic!("{e}"));
    serde_json::from_value(started["job"].clone()).unwrap_or_else(|e| panic!("{e}"))
}

/// `params` run as `id` in Warn: the reply, which must match `schema` and have no warnings.
fn reply(s: &mut Session, id: &str, params: Value, schema: &schemars::Schema) -> Value {
    let r = s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id}: {r} doesn't match {}", schema.as_value());
    assert!(r.get("param_warnings").is_none(), "{id}: {r}");
    r
}

#[test]
fn schemas_and_the_key_check_agree() {
    for schema in
        [input_schema_for::<StatusParams>(), input_schema_for::<CancelParams>(), input_schema_for::<PinParams>(), input_schema_for::<CopyParams>()]
    {
        assert_keys(schema, &["job"]);
    }
    assert_keys(input_schema_for::<RegradeParams>(), &["job", "presentation"]);
    assert_keys(input_schema_for::<ExportParams>(), &["format", "job", "overwrite", "path"]);
    assert_eq!(input_schema_for::<super::ExportFormat>().as_value()["enum"], json!(["png", "exr", "denoise_mask"]));
}

/// #12836: `JobPhase` names every phase the render crate has, as it spells them. The match has
/// no catch-all, so a new phase stops this test from compiling until `JobPhase` has it too.
#[test]
fn job_phase_names_every_render_phase() {
    let phases =
        [Phase::Queued, Phase::Preparing, Phase::Running, Phase::Encoding, Phase::Cancelling, Phase::Cancelled, Phase::Completed, Phase::Failed];
    for phase in phases {
        let ours = match phase {
            Phase::Queued => JobPhase::Queued,
            Phase::Preparing => JobPhase::Preparing,
            Phase::Running => JobPhase::Running,
            Phase::Encoding => JobPhase::Encoding,
            Phase::Cancelling => JobPhase::Cancelling,
            Phase::Cancelled => JobPhase::Cancelled,
            Phase::Completed => JobPhase::Completed,
            Phase::Failed => JobPhase::Failed,
        };
        assert_eq!(json!(phase), json!(ours));
    }
}

/// #12836: calls that fail today get the same message in strict: serde's for a job, a
/// presentation, a path, a format or `overwrite` of the wrong shape (the check passes it on
/// whole), and the handlers' own for a job of another design, a presentation out of range, a
/// path without the format's extension and no result to use. (A key left out is in
/// `a_key_left_out_is_refused_by_the_check`, an unknown key in
/// `an_unknown_key_is_refused_in_every_mode`.)
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("render.status", json!({"job": 5})),
        ("render.status", json!({"job": {"session": 1}})),
        ("render.status", json!({"job": {"session": 1, "serial": 2, "generation": 3, "shot": 4}})),
        ("render.status", json!({"job": {"session": -1, "serial": 2, "generation": 3}})),
        ("render.status", json!({"job": {"session": 1.5, "serial": 2, "generation": 3}})),
        ("render.status", json!({"job": foreign_job()})),
        ("render.cancel", json!({"job": "Shot 1"})),
        ("render.cancel", json!({})),
        ("render.result.pin", json!({"job": [1, 2, 3]})),
        ("render.result.pin", json!({})),
        ("render.result.copy", json!({"job": true})),
        ("render.result.copy", json!({})),
        ("render.result.regrade", json!({"presentation": 5})),
        ("render.result.regrade", json!({"presentation": {"exposure": 1}})),
        ("render.result.regrade", json!({"presentation": {"tone_map": "filmic"}})),
        ("render.result.regrade", json!({"presentation": {"background": [256, 0, 0]}})),
        ("render.result.regrade", json!({"presentation": {"backdrop": {"mode": "diagonal"}}})),
        ("render.result.regrade", json!({"presentation": {"exposure_ev": 40}})),
        ("render.result.regrade", json!({"presentation": {}})),
        ("render.export", json!({"path": 5})),
        ("render.export", json!({"path": "shot.png", "format": "jpg"})),
        ("render.export", json!({"path": "shot.png", "format": null})),
        ("render.export", json!({"path": "shot.png", "overwrite": "yes"})),
        ("render.export", json!({"path": "shot.txt"})),
        ("render.export", json!({"path": "mask.exr", "format": "denoise_mask"})),
        ("render.export", json!({"path": "shot.png", "job": foreign_job()})),
    ]);
}

/// #12836: Regrade Linear Master without `presentation` and Export Render Image without `path`
/// are refused by the check itself: in strict with the check's "is required", where the
/// handler says serde's "missing field".
#[test]
fn a_key_left_out_is_refused_by_the_check() {
    for (id, p, key) in [("render.result.regrade", json!({}), "presentation"), ("render.export", json!({"format": "png"}), "path")] {
        let messages: Vec<String> = crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect();
        assert_eq!(messages, [format!("`{key}` is required")], "{id} {p}");
        let error = |level: ParamCheck| Session { param_check: level, ..Session::default() }.execute(id, &p).err().map(|e| e.to_string());
        assert!(error(ParamCheck::Off).is_some_and(|e| e.ends_with(&format!("missing field `{key}`"))), "{id}: {:?}", error(ParamCheck::Off));
        assert!(error(ParamCheck::Strict).is_some_and(|e| e.ends_with(&format!("`{key}` is required"))), "{id}: {:?}", error(ParamCheck::Strict));
    }
}

/// #12836: an unknown key fails in every mode, as the handlers' serde structs refuse it: with
/// serde's message with the check off, and in strict with the check's, which suggests `job`.
#[test]
fn an_unknown_key_is_refused_in_every_mode() {
    for id in ["render.status", "render.cancel", "render.result.pin", "render.result.copy", "render.result.regrade", "render.export"] {
        let p = json!({"jobs": null});
        let error = |level: ParamCheck| Session { param_check: level, ..Session::default() }.execute(id, &p).err().map(|e| e.to_string());
        assert!(error(ParamCheck::Off).is_some_and(|e| e.contains("unknown field `jobs`")), "{id}: {:?}", error(ParamCheck::Off));
        assert!(error(ParamCheck::Strict).is_some_and(|e| e.contains("did you mean `job`?")), "{id}: {:?}", error(ParamCheck::Strict));
    }
}

/// #12836: the contracts take what the input schemas allow: each call's check finds no issue
/// exactly when it matches the schema. (What the handlers refuse after parsing, such as an
/// exposure of 40 stops or a `.txt` path, both allow.)
#[test]
fn the_checks_take_what_the_schemas_allow() {
    let job = json!({"session": 1, "serial": 2, "generation": 3});
    let cases = [
        ("render.status", json!({}), true),
        ("render.status", json!({"job": null}), true),
        ("render.status", json!({"job": job}), true),
        ("render.status", json!({"job": 5}), false),
        ("render.status", json!({"job": {"session": 1}}), false),
        ("render.status", json!({"job": {"session": 1, "serial": 2, "generation": 3, "shot": 4}}), false),
        ("render.status", json!({"job": {"session": -1, "serial": 2, "generation": 3}}), false),
        ("render.status", json!({"job": {"session": 1.5, "serial": 2, "generation": 3}}), false),
        ("render.result.pin", json!({"job": null}), true),
        ("render.result.regrade", json!({"presentation": {}}), true),
        (
            "render.result.regrade",
            json!({"job": job, "presentation": {"exposure_ev": 1.5, "tone_map": "aces_fitted", "background": [1, 2, 3], "transparent_background": true}}),
            true,
        ),
        ("render.result.regrade", json!({"presentation": {"exposure_ev": 40}}), true),
        ("render.result.regrade", json!({"presentation": {"backdrop": null}}), true),
        ("render.result.regrade", json!({"presentation": {"backdrop": {"mode": "radial", "center": [0.5, 0.5], "radius": 0.5}}}), true),
        ("render.result.regrade", json!({"presentation": {"finishing": {"contrast": 1.1, "looks": [], "bloom": null}}}), true),
        ("render.result.regrade", json!({"presentation": {"background": [256, 0, 0]}}), false),
        ("render.result.regrade", json!({"presentation": {"tone_map": "filmic"}}), false),
        ("render.result.regrade", json!({"presentation": {"exposure": 1}}), false),
        ("render.result.regrade", json!({"presentation": {"finishing": {"glow": 1}}}), false),
        ("render.result.regrade", json!({"presentation": {"backdrop": {"mode": "diagonal"}}}), false),
        ("render.result.regrade", json!({"presentation": null}), false),
        ("render.export", json!({"path": "shot.png"}), true),
        ("render.export", json!({"path": "shot.txt"}), true),
        ("render.export", json!({"path": "shot.exr", "format": "exr", "overwrite": true, "job": null}), true),
        ("render.export", json!({"path": "mask.png", "format": "denoise_mask", "job": job}), true),
        ("render.export", json!({"path": 5}), false),
        ("render.export", json!({"path": "shot.png", "format": "PNG"}), false),
        ("render.export", json!({"path": "shot.png", "format": null}), false),
        ("render.export", json!({"path": "shot.png", "overwrite": null}), false),
    ];
    let schemas = crate::cmd::command_specs().into_iter().filter_map(|c| c.contract.map(|contract| (c.id, contract.input_schema())));
    let schemas: std::collections::HashMap<_, _> = schemas.collect();
    for (id, p, takes) in cases {
        let schema = schemas.get(id).unwrap_or_else(|| panic!("{id} has no contract")).as_value();
        assert_eq!(crate::schema::issues(id, &p).is_empty(), takes, "{id} {p}: {:?}", crate::schema::issues(id, &p));
        assert_eq!(conforms(schema, schema, &p), takes, "{id} {p}");
    }
}

/// #12836: each command replies as its output schema says, in Warn without warnings, and the
/// replies are the JSON the handlers gave before: a finished job's status and the queue, the
/// result selected and cleared, the image copied and exported, a finished job left as it is
/// and a running job cancelled.
#[test]
fn render_job_commands_reply_with_their_schemas() {
    let host = CpuHost::cooperative();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap_or_else(|e| panic!("{e}"));
    let job = start(&mut s);
    finish(&host);
    s.param_check = ParamCheck::Warn;

    let progress = output_schema_for::<RenderProgress>();
    let state = reply(&mut s, "render.status", json!({"job": job}), &progress);
    let mut before = json!(s.render_jobs.status(job).unwrap_or_else(|e| panic!("{e}")));
    before["queue"] = json!(s.render_queue_state());
    assert_eq!(state, before);
    assert_eq!((&state["progress"]["phase"], &state["frame"]["width"]), (&json!("completed"), &json!(16)), "{state}");
    let queue = reply(&mut s, "render.status", json!({}), &progress);
    assert_eq!(queue, json!({"queue": {"running": 0, "waiting": 0, "job": null, "shot": null, "error": null}}));

    let pinned = output_schema_for::<ResultPinned>();
    assert_eq!(reply(&mut s, "render.result.pin", json!({"job": job}), &pinned), json!({"frame": state["frame"]}));
    let copied = reply(&mut s, "render.result.copy", json!({}), &output_schema_for::<ImageCopied>());
    assert_eq!(copied, json!({"frame": state["frame"]}));
    let path = std::env::temp_dir().join(format!("solidcraft-render-jobs-schema-{}.png", std::process::id()));
    let exported = reply(&mut s, "render.export", json!({"job": job, "path": path, "overwrite": true}), &output_schema_for::<ImageExported>());
    let written = std::fs::metadata(&path).map(|m| m.len()).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        exported,
        json!({"job": job, "path": path, "bytes": written, "format": "png", "encoding": "srgb", "alpha": "straight",
            "frame": state["frame"], "linear_master": null, "presentation": null, "denoise_mask": null})
    );
    assert_eq!(reply(&mut s, "render.result.pin", json!({"job": null}), &pinned), json!({"frame": null}));

    let cancelled = output_schema_for::<RenderCancelled>();
    let finished = reply(&mut s, "render.cancel", json!({"job": job}), &cancelled);
    assert_eq!(finished, json!(s.render_jobs.status(job).unwrap_or_else(|e| panic!("{e}"))));
    assert_eq!(finished["progress"]["phase"], "completed", "a finished job is left as it is");
    let running = start(&mut s);
    let stopping = reply(&mut s, "render.cancel", json!({"job": running}), &cancelled);
    assert!(["cancelling", "cancelled"].contains(&stopping["progress"]["phase"].as_str().unwrap_or_default()), "{stopping}");
    finish(&host);
}

/// #12836: Regrade Linear Master replies as its output schema says, and so does Export Render
/// Image for the linear master (EXR), the regraded colour PNG and the retained guide masks.
/// Every export preserves the parent colour frame, master, presentation and mask metadata.
#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn regrade_and_export_of_a_linear_master_reply_with_their_schemas() {
    let mut s = Session::default();
    s.execute(
        "render.settings.set",
        &json!({"name": "Master", "set": {"width": 8, "height": 8, "job": {"method": "cpu_raytrace",
            "options": {"progressive": {"samples_per_pixel": 1, "seed": 7}, "lights": [], "denoise_diagnostics": true,
                "output": {"linear_exr": true}}}}}),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap_or_else(|e| panic!("{e}")))).unwrap_or_else(|e| panic!("{e}"));
    let wait = |s: &Session, job: JobId| {
        let status = s.render_job_handle(job).and_then(|h| h.wait_timeout(std::time::Duration::from_secs(30)).map_err(crate::render_jobs::job_error));
        assert_eq!(status.map(|status| status.progress.phase).ok(), Some(Phase::Completed));
    };
    let started = s.execute("render.start", &json!({"setup": "Master"})).unwrap_or_else(|e| panic!("{e}"));
    let master: JobId = serde_json::from_value(started["job"].clone()).unwrap_or_else(|e| panic!("{e}"));
    wait(&s, master);
    s.param_check = ParamCheck::Warn;

    let regraded =
        reply(&mut s, "render.result.regrade", json!({"job": master, "presentation": {"exposure_ev": -1}}), &output_schema_for::<super::JobState>());
    let regraded: JobId = serde_json::from_value(regraded["job"].clone()).unwrap_or_else(|e| panic!("{e}"));
    wait(&s, regraded);
    let exported = output_schema_for::<ImageExported>();
    for (job, format, extension) in
        [(master, "exr", "exr"), (regraded, "png", "png"), (master, "denoise_mask", "png"), (regraded, "denoise_mask", "png")]
    {
        let frame = s.render_jobs.frame(job).unwrap_or_else(|e| panic!("{e}"));
        let mask_info = json!(frame.denoise_mask_info().unwrap_or_else(|| panic!("missing retained guide mask")));
        let expected = match format {
            "exr" => frame.linear_exr().unwrap_or_else(|| panic!("missing retained linear master")),
            "denoise_mask" => frame.denoise_mask_png().unwrap_or_else(|| panic!("missing retained guide mask")),
            _ => frame.png(),
        };
        let path = std::env::temp_dir().join(format!("solidcraft-render-jobs-schema-{}-{}-{format}.{extension}", std::process::id(), job.serial));
        let r = reply(&mut s, "render.export", json!({"job": job, "path": path, "format": format, "overwrite": true}), &exported);
        assert_eq!(std::fs::read(&path).unwrap_or_else(|e| panic!("{e}")), expected);
        let _ = std::fs::remove_file(&path);
        assert_eq!(r["format"], format);
        assert_eq!(r["frame"], json!(frame.info));
        assert_eq!(r["denoise_mask"], mask_info);
        let (encoding, alpha) = match format {
            "exr" => ("scene_linear_srgb_d65", "coverage_premultiplied"),
            "denoise_mask" => ("categorical_srgb", "opaque"),
            _ => ("srgb", "straight"),
        };
        assert_eq!(r["encoding"], encoding);
        assert_eq!(r["alpha"], alpha);
        assert!(r["linear_master"].is_object(), "{r}");
        if job == regraded {
            assert!(r["presentation"].is_object(), "{r}");
        }
    }
}
