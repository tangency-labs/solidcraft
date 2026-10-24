//! Focused integration acceptance through real public engine commands. Avoids linking
//! the enormous full engine test binary for every resource/UI-adapter change.
//! Original analytic fixture; no external CAD or image data. New output folder only.
use serde_json::{Value, json};
use solidcraft_engine::{
    Session,
    doc::Document,
    render::job::{CpuHost, Phase},
};
use std::{error::Error, fs, path::PathBuf, time::Duration};
fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition { Ok(()) } else { Err(message.into()) }
}
fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os().nth(1).map(PathBuf::from).ok_or("usage: render_resource_workflow NEW_OUTPUT_DIRECTORY")?;
    fs::create_dir(&output)?;
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":20,"width":15,"height":4,"body_name":"Coupon"}))?;
    let design = session.doc.clone();
    let binding = session.execute(
        "render.library.surface.bind",
        &json!({"name":"Mapped","set":{"width":96,"height":64,"job":{"method":"gpu_path","options":{}}},
        "surface":{"id":"SURF-001","body_index":0,"body_name":"Coupon"}}),
    )?;
    require(session.doc.render.setups["Mapped"].surfaces.len() == 1, "missing saved surface")?;
    require(
        session.doc.features == design.features && session.doc.materials == design.materials && session.doc.appearances == design.appearances,
        "surface binding changed engineering/appearance authority",
    )?;
    let saved = session.doc.to_json();
    require(
        session
            .execute(
                "render.library.surface.bind",
                &json!({"name":"Mapped","set":{"surfaces":[]},
        "surface":{"id":"SURF-002","body_index":0,"body_name":"Coupon"}}),
            )
            .is_err(),
        "a setup patch bypassed surface replacement consent",
    )?;
    require(session.doc.to_json() == saved, "failed surface reset mutated the design")?;
    let reopened = Document::from_json(&saved)?;
    require(reopened.render.setups["Mapped"] == session.doc.render.setups["Mapped"], "surface setup changed on reopen")?;
    let mut restored = Session::new(reopened);
    restored.execute("render.scene", &json!({}))?;
    let before = restored.doc.to_json();
    require(
        restored.execute("render.settings.set", &json!({"name":"Mapped","set":{"job":{"method":"cpu_raytrace","options":{}}}})).is_err(),
        "unsupported CPU maps were silently discarded",
    )?;
    require(restored.doc.to_json() == before, "failed method change mutated saved design")?;
    let target = restored.doc.render.setups["Mapped"].surfaces.iter().next().ok_or("surface target missing")?.target.clone();
    restored.execute("render.library.surface.unbind", &json!({"name":"Mapped","target":target}))?;
    require(restored.doc.render.setups["Mapped"].surfaces.is_empty(), "explicit removal failed")?;
    restored.execute("edit.undo", &json!({}))?;
    require(restored.doc.to_json() == before, "surface unbind undo did not restore exact saved intent")?;

    let profiles = restored.execute("render.library.list", &json!({"kind":"physical_profiles","limit":128}))?;
    let mut planned = 0;
    let mut refused = 0;
    let mut checked = 0;
    for item in profiles["items"].as_array().ok_or("profile list missing")? {
        let id = item["id"].as_str().ok_or("profile ID missing")?;
        let detail = restored.execute("render.library.describe", &json!({"kind":"physical_profiles","id":id}))?;
        require(detail["physical_assignment"] == false && detail["document_changed"] == false, "physical reference lookup mutates design")?;
        checked += 1;
        match restored.execute("render.library.physical.prepare", &json!({"id":id,"name":"Reviewed reference","acknowledge_reference":true})) {
            Ok(plan) => {
                require(plan["status"] == "plan_only_not_executed", "physical preparation executed a command")?;
                planned += 1;
            }
            Err(_) => refused += 1,
        }
    }
    require((checked, planned, refused) == (59, 46, 13), "unexpected physical reference/point-density coverage")?;
    require(restored.doc.to_json() == before, "physical planning altered the design")?;
    let complete = restored
        .execute("render.library.studio.prepare", &json!({"id":"RIG-001","variant":"complete","center_mm":[10,20,30],"subject_extent_mm":100}))?;
    require(
        complete["projection"]["passive_cards_included"] == 1 && complete["projection"]["passive_cards_omitted"] == 0,
        "complete rig dropped passive fill",
    )?;
    require(complete["stage"][0]["center_mm"] == json!([90.0, 10.0, 30.0]), "rig scale applied incorrectly")?;

    let host = CpuHost::for_platform()?;
    restored.set_render_job_host(Some(host.clone()))?;
    let stage = restored.execute(
        "render.stage.ground",
        &json!({"name":"Ground","set":{"width":96,"height":64,"job":{"method":"cpu_raytrace","options":{"budget":"studio"}}}}),
    )?;
    require(
        restored.execute("render.stage.ground", &json!({"name":"Ground","set":{"stage":[]}})).is_err(),
        "a setup patch bypassed stage replacement consent",
    )?;
    let hdr_request = json!({"id":"HDR-001","name":"HDR","set":{"job":{"method":"cpu_raytrace","options":{}}}});
    restored.execute("render.library.environment.apply", &hdr_request)?;
    require(
        restored.execute("render.library.environment.apply", &hdr_request).is_err(),
        "a full job patch bypassed environment replacement consent",
    )?;
    let preflight = restored.execute("render.preflight", &json!({"setup":"Ground"}))?;
    require(preflight["stage_panels"] == 1, "preflight omitted saved ground")?;
    require(
        restored.doc.features == design.features && restored.doc.materials == design.materials,
        "staging changed CAD geometry or physical material",
    )?;
    let started = restored.execute("render.start", &json!({"setup":"Ground"}))?;
    let id = serde_json::from_value(started["job"].clone())?;
    let handle = restored.render_job_handle(id)?;
    let status = match handle.wait_timeout(Duration::from_secs(120)) {
        Ok(status) => status,
        Err(error) => {
            let _ = handle.cancel();
            return Err(error.into());
        }
    };
    require(status.progress.phase == Phase::Completed, "staged CPU render failed")?;
    let frame = handle.frame()?.ok_or("completed staged render missing frame")?;
    require(frame.info.width == 96 && frame.info.height == 64, "staged output dimensions changed")?;
    fs::write(output.join("cpu-passive-ground.png"), frame.png())?;
    let report: Value = json!({"status":"PASS","native_command_workflow":true,"ui_tested":false,"whole_selene_rendered":false,
        "surface_binding":binding,"physical_profiles_checked":checked,"point_density_plans":planned,"missing_density_refused":refused,
        "complete_rig":complete,"ground_setup":stage,"preflight":preflight,"completed_frame":frame.info,
        "unchanged_engineering_geometry":true,"physical_profiles_assigned":false});
    fs::write(output.join("receipt.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
