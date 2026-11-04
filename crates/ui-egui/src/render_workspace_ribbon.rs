//! Interactive adapters for existing registered commands, shared by ribbon,
//! menus, search and pinned job controls. Direct engine JSON APIs are unchanged.
use super::{cpu, inspector, library, setups, studio};
use crate::SolveApp;
use serde_json::json;
use solidcraft_engine::render::job::Phase;
use solidcraft_engine::{CommandInfo, CommandSpec};

fn is_job_action(id: &str) -> bool {
    matches!(
        id,
        "render.preflight" | "render.start" | "render.batch" | "render.cancel" | "render.export" | "render.result.regrade" | "render.result.copy"
    )
}
fn handled(id: &str) -> bool {
    is_job_action(id)
        || matches!(
            id,
            "render.aging.describe"
                | "render.effects.describe"
                | "render.environment.set"
                | "render.preset.resolve"
                | "render.preset.apply"
                | "render.settings.get"
                | "render.settings.set"
                | "render.shot.duplicate"
                | "render.shot.rename"
                | "render.shot.remove"
                | "render.library.appearance.apply"
                | "render.optics.bind"
                | "render.optics.unbind"
                | "render.library.surface.bind"
                | "render.library.surface.unbind"
                | "render.library.surface.prepare"
                | "render.library.physical.prepare"
                | "render.library.studio.prepare"
                | "render.library.studio.apply"
                | "render.library.environment.apply"
                | "render.stage.ground"
                | "render.stage.backdrop"
                | "render.stage.clear"
                | "render.hdr.inspect"
                | "render.settings.capture_hdr"
                | "render.settings.apply"
                | "render.camera.fit"
                | "render.camera.get"
                | "render.camera.set"
                | "render.quality.prepare"
                | "render.status"
                | "render.backends"
                | "render.result.view"
                | "render.result.close"
                | "render.compare"
        )
}

pub(super) fn busy(app: &SolveApp) -> bool {
    let queue = app.session.render_queue_state();
    if queue.running != 0 || queue.waiting != 0 {
        return true;
    }
    app.render_workspace
        .view
        .job
        .and_then(|job| app.session.render_job_handle(job).ok())
        .and_then(|job| job.status().ok())
        .is_some_and(|s| matches!(s.progress.phase, Phase::Queued | Phase::Preparing | Phase::Running | Phase::Encoding | Phase::Cancelling))
}

/// Reads only current host/job metadata and the numerical draft. No preflight,
/// scene preparation, device discovery, resource IO or rendering during painting.
pub(super) fn action_ready(app: &SolveApp, id: &str) -> Result<(), String> {
    if app.file_load.busy() || app.file_picker.is_some() || inspector::confirmation_open(app) {
        return Err(app.tr("render.ribbon_finish_dialog").into());
    }
    match id {
        "render.batch" => {
            if app.session.doc.render.setups.is_empty() {
                return Err(app.tr("render.batch_empty").into());
            }
            if app.session.doc.render.setups.len() > 32 {
                return Err(app.tr("render.batch_limit").into());
            }
            if busy(app) {
                return Err(app.tr("render.ribbon_job_active").into());
            }
            super::image_params(app).map(|_| ())
        }
        "render.compare" if app.render_workspace.view.retained.is_empty() => Err(app.tr("render.ribbon_no_result").into()),
        "render.preflight" | "render.start" => {
            if id == "render.start" && busy(app) {
                return Err(app.tr("render.ribbon_job_active").into());
            }
            super::image_params(app).map(|_| ())
        }
        "render.result.regrade" => {
            if busy(app) {
                return Err(app.tr("render.ribbon_job_active").into());
            }
            if !studio::linear_export_available(app) {
                return Err(app.tr("render.linear_unavailable").into());
            }
            presentation(app).validate().map_err(|error| error.to_string())
        }
        "render.cancel" if !busy(app) => Err(app.tr("render.ribbon_no_active_job").into()),
        "render.result.close" if !super::result_active(app) => Err(app.tr("render.ribbon_no_result").into()),
        "render.export" | "render.result.copy" => {
            let completed = app
                .render_workspace
                .view
                .completed
                .and_then(|id| app.session.render_job_handle(id).ok())
                .and_then(|handle| handle.frame().ok().flatten());
            if completed.is_none() {
                return Err(app.tr("render.ribbon_no_result").into());
            }
            if id == "render.export" && app.render_workspace.view.studio.export_exr && !studio::linear_export_available(app) {
                return Err(app.tr("render.linear_unavailable").into());
            }
            Ok(())
        }
        "render.settings.set" => setups::snapshot(app).map(|_| ()),
        "render.settings.apply" if !app.session.doc.render.setups.contains_key(&app.render_workspace.view.name) => {
            Err(app.tr("render.ribbon_no_saved_camera").into())
        }
        _ => Ok(()),
    }
}

pub(super) fn command_info(app: &SolveApp, command: &CommandSpec) -> CommandInfo {
    // Play History swaps to Stop History while it runs, wherever the command is shown.
    if command.id == "timeline.play" {
        return crate::timeline::playback_command_info(app, command);
    }
    let mut info = command.info(&app.session);
    if info.enabled
        && super::active(app)
        && handled(command.id)
        && let Err(reason) = action_ready(app, command.id)
    {
        info.enabled = false;
        info.disabled_reason = Some(reason);
    }
    info
}

/// Execute the same explicit request from either the ribbon or the sticky footer.
/// No action falls back to a default executor, saved shot or a different result.
pub(super) fn perform_job(app: &mut SolveApp, id: &str) -> Result<(), String> {
    action_ready(app, id)?;
    match id {
        "render.batch" => {
            let request = super::image_params(app)?;
            app.run(id, json!({"request":request}))?;
        }
        "render.preflight" => {
            let request = super::image_params(app)?;
            let reply = match app.run(id, request.clone()) {
                Ok(reply) => reply,
                Err(error) => {
                    inspector::record_check(app, &request, &json!({"error":error}));
                    return Err(error);
                }
            };
            cpu::record_checked(app, &request, &reply);
            inspector::record_check(app, &request, &reply);
            app.set_status(app.tr("render.image_supported").to_string(), false);
        }
        "render.start" => {
            let request = super::image_params(app)?;
            let reply = app.run(id, request)?;
            let job = serde_json::from_value(reply["job"].clone())
                .map_err(|e| crate::i18n::format("render.error_job_identity", &[("error", &e.to_string())]))?;
            app.render_workspace.view.job = Some(job);
            app.render_workspace.view.job_shot = Some(app.render_workspace.view.name.clone());
        }
        "render.result.copy" => {
            let job = app.render_workspace.view.completed.ok_or_else(|| app.tr("render.ribbon_no_result").to_string())?;
            app.run(id, json!({"job":job}))?;
        }
        "render.result.regrade" => {
            let source = app.render_workspace.view.completed.ok_or_else(|| app.tr("render.ribbon_no_result").to_string())?;
            let reply = app.run(id, json!({"job":source,"presentation":presentation(app)}))?;
            let job = serde_json::from_value(reply["job"].clone())
                .map_err(|error| crate::i18n::format("render.error_regrade_identity", &[("error", &error.to_string())]))?;
            app.render_workspace.view.job = Some(job);
        }
        "render.cancel" => {
            let job = app
                .session
                .render_queue_state()
                .job
                .or(app.render_workspace.view.job)
                .ok_or_else(|| app.tr("render.ribbon_no_active_job").to_string())?;
            app.run(id, json!({"job":job}))?;
        }
        "render.export" => {
            let job = app.render_workspace.view.completed.ok_or_else(|| app.tr("render.ribbon_no_result").to_string())?;
            let overwrite = app.render_workspace.view.overwrite;
            if app.render_workspace.view.studio.export_exr {
                super::export_linear_image(app, job, overwrite);
            } else {
                super::export_image(app, job, overwrite);
            }
        }
        _ => return Err(crate::i18n::tr("render.error_job_action").into()),
    }
    Ok(())
}

pub(super) fn presentation(app: &SolveApp) -> solidcraft_engine::render::job::presentation::Presentation {
    let view = &app.render_workspace.view;
    solidcraft_engine::render::job::presentation::Presentation {
        exposure_ev: view.draft.exposure_ev,
        tone_map: view.studio.tone_map,
        finishing: view.studio.finishing,
        backdrop: view.studio.backdrop_enabled.then_some(view.studio.backdrop),
        background: view.draft.background,
        transparent_background: view.transparent,
    }
}

fn focus(app: &mut SolveApp, selection: super::RenderSelection, category: Option<&str>) {
    use super::RenderSelection;
    if matches!(selection, RenderSelection::Object(_)) {
        // Object shortcuts keep the shared modelling selection.
        super::selection_editor(app, &selection);
    } else {
        let kind = match &selection {
            RenderSelection::Nothing => "nothing",
            RenderSelection::Environment => "environment",
            RenderSelection::Wear => "wear",
            RenderSelection::Lighting => "lighting",
            RenderSelection::Light(_) => "light",
            RenderSelection::Shot(_) => "shot",
            RenderSelection::Output => "output",
            RenderSelection::Sampling => "sampling",
            RenderSelection::Effects => "effects",
            RenderSelection::Check => "check",
            RenderSelection::Object(_) => return,
        };
        let mut params = json!({"kind":kind});
        if let RenderSelection::Shot(id) | RenderSelection::Light(id) = &selection {
            params["id"] = json!(id);
        }
        let _ = app.run("view.render_select", params);
    }
    if let Some(category) = category {
        library::open_category(app, category);
    }
}

/// Parameterless command activation opens the already implemented editor. The
/// engine's explicit resolve/apply methods still require their original params.
pub(super) fn start(app: &mut SolveApp, id: &str) -> bool {
    if !handled(id) {
        return false;
    }
    if !super::active(app) {
        if let Err(error) = super::enter(app, true) {
            app.set_status(error, true);
            return true;
        }
        // Entering from Design never starts an expensive render with hidden defaults.
        focus(app, super::RenderSelection::Output, None);
        if is_job_action(id) {
            return true;
        }
    }
    let result = if is_job_action(id) { perform_job(app, id) } else { navigate(app, id) };
    if let Err(error) = result {
        app.set_status(error, true);
    }
    true
}

fn navigate(app: &mut SolveApp, id: &str) -> Result<(), String> {
    action_ready(app, id)?;
    use super::RenderSelection;
    let object = super::object_selection(app);
    let shot = super::current_shot_selection(app);
    match id {
        "render.aging.describe" => {
            let _ = app.run("view.render_select", json!({"kind":"wear"}));
        }
        "render.compare" => super::compare::start_default(app)?,
        "render.result.close" => {
            app.run(id, json!({}))?;
        }
        "render.result.view" => {
            app.run(id, json!({"view":"image"}))?;
        }
        "render.effects.describe" => focus(app, RenderSelection::Effects, None),
        "render.preset.resolve" | "render.preset.apply" => focus(app, RenderSelection::Nothing, Some("native_scenes")),
        "render.settings.get" | "render.shot.duplicate" | "render.shot.rename" | "render.shot.remove" => {
            focus(app, shot, None);
        }
        "render.settings.set" => app.render_workspace.view.save_shot_name = Some(String::new()),
        "render.library.appearance.apply" | "render.optics.bind" | "render.optics.unbind" => focus(app, object, Some("surfaces")),
        "render.library.surface.bind" | "render.library.surface.prepare" | "render.library.surface.unbind" => {
            focus(app, object, Some("surfaces"));
        }
        "render.library.physical.prepare" => focus(app, object, Some("physical_profiles")),
        "render.library.studio.prepare" => focus(app, RenderSelection::Lighting, Some("rigs")),
        "render.library.studio.apply" => focus(app, RenderSelection::Lighting, None),
        "render.library.environment.apply" => focus(app, RenderSelection::Environment, Some("hdr")),
        "render.environment.set" | "render.hdr.inspect" | "render.settings.capture_hdr" => focus(app, RenderSelection::Environment, None),
        "render.stage.backdrop" => focus(app, RenderSelection::Environment, None),
        "render.stage.ground" | "render.stage.clear" => {
            focus(app, RenderSelection::Environment, None);
            inspector::reveal(app, "render_ground_details");
        }
        "render.settings.apply" => inspector::restore_camera(app)?,
        "render.camera.fit" => inspector::fit_subject(app)?,
        "render.camera.get" | "render.camera.set" => focus(app, shot, None),
        "render.quality.prepare" => {
            focus(app, RenderSelection::Sampling, Some("quality"));
            library::review_selected_quality(app)?;
        }
        "render.status" | "render.backends" => focus(app, RenderSelection::Check, None),
        _ => return Err(crate::i18n::tr("render.error_interactive_command").into()),
    }
    Ok(())
}
