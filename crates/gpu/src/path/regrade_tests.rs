use super::*;
use serde_json::json;
use solidcraft_render::job::{JobId, Phase};

#[test]
fn engine_regrades_retained_master_after_geometry_changes_without_new_transport_or_design_edits() {
    let Some(executor) = executor() else { return };
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let mut session = solidcraft_engine::Session::default();
    session.register_render_job_host(host).unwrap();
    session.execute("solid.box", &json!({"length":2,"width":2,"height":1,"body_name":"Captured subject"})).unwrap();
    let input = json!({"progressive":true,"samples":32,"linear_exr":true,"settings":{"width":16,"height":12},
        "adaptive":{"pilot_samples":16,"min_samples":16,"relative_error":0.1,"absolute_error":0.01,"coverage_error":0.02},
        "transmission_samples":1,"finishing":{"contrast":1.2}});
    let check = session.execute("render.preflight", &input).unwrap();
    assert_eq!(check["options"]["adaptive"]["pilot_samples"], 16);
    let started = session.execute("render.start", &input).unwrap();
    let source: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let source_handle = session.render_job_handle(source).unwrap();
    let status = source_handle.wait_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    assert!(status.sampling.is_some());
    let original = source_handle.frame().unwrap().unwrap();
    session.execute("solid.box", &json!({"corner":[8,0,0],"length":1,"width":1,"height":1,"operation":"new","body_name":"Later geometry"})).unwrap();
    let document = session.doc.to_json();
    let revision = session.revision;
    let presentation = json!({"exposure_ev":-2.0,"finishing":{"saturation":0.0},"backdrop":{"mode":"radial"}});
    let graded = session.execute("render.result.regrade", &json!({"job":source,"presentation":presentation})).unwrap();
    let job: JobId = serde_json::from_value(graded["job"].clone()).unwrap();
    assert_ne!(job, source);
    let handle = session.render_job_handle(job).unwrap();
    let status = handle.wait_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    assert_eq!(status.resolved.executor.as_str(), "linear-master");
    assert_eq!(status.scene_generation, original.info.scene_generation);
    assert_eq!(status.presentation.as_ref().unwrap().master_job, source);
    assert_eq!(status.presentation.as_ref().unwrap().master_sha256, original.linear_master_info().unwrap().sha256);
    let result = handle.frame().unwrap().unwrap();
    assert_eq!(result.linear_exr().unwrap().as_ptr(), original.linear_exr().unwrap().as_ptr());
    assert_ne!(result.png(), original.png());
    assert_eq!(session.doc.to_json(), document);
    assert_eq!(session.revision, revision);
    assert!(session.execute("render.result.regrade", &json!({"job":JobId{session:source.session+1,..source},"presentation":presentation})).is_err());
    assert!(session.execute("render.result.regrade", &json!({"job":job,"presentation":{"denoise":true}})).is_err());
    assert_eq!(session.doc.to_json(), document);
}
