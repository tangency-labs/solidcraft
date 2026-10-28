//! Captured review plans. Preparing/closing never mutates the design.
use super::*;

#[derive(Clone, Debug)]
pub(in crate::render_workspace) enum Apply {
    Command { id: String, params: Value },
    Maps { id: String, target: materials::Target, plane: String, scale: f64 },
    Quality(Value),
    Physical(Value),
}

#[derive(Clone, Debug)]
pub(in crate::render_workspace) struct Draft {
    pub variant: &'static str,
    pub name: String,
    pub plan: Value,
    pub action: Apply,
    pub acknowledged: bool,
    pub bounds: bool,
    pub emitters_only: bool,
    pub center: [f64; 3],
    pub extent: f64,
    revision: u64,
    document: Weak<Document>,
    snapshot: Value,
    shot: String,
    method: ImageMethod,
}

impl Draft {
    fn current(&self, app: &SolveApp) -> bool {
        self.revision == app.session.revision
            && self.shot == app.render_workspace.view.name
            && self.method == app.render_workspace.view.method
            && Weak::ptr_eq(&self.document, &Arc::downgrade(&app.session.doc))
            && patch(app).ok().as_ref() == Some(&self.snapshot)
    }
}

pub(in crate::render_workspace) fn open(app: &mut SolveApp, variant: &'static str, name: String, plan: Value, action: Apply) -> Result<(), String> {
    let snapshot = patch(app)?;
    app.render_workspace.view.library.review = Some(Draft {
        variant,
        name,
        plan,
        action,
        acknowledged: false,
        bounds: true,
        emitters_only: false,
        center: [0.0; 3],
        extent: 1000.0,
        revision: app.session.revision,
        document: Arc::downgrade(&app.session.doc),
        snapshot,
        shot: app.render_workspace.view.name.clone(),
        method: app.render_workspace.view.method,
    });
    Ok(())
}

pub(in crate::render_workspace) fn quality(app: &mut SolveApp, id: &str) -> Result<(), String> {
    if app.render_workspace.view.method != ImageMethod::GpuRaytrace {
        return Err(app.tr("render.stage_panels.gpu_required").into());
    }
    let plan = app.run("render.quality.prepare", json!({"id":id,"variant":"fixed_samples_png","acknowledge_omissions":true}))?;
    open(app, "quality", plan["name"].as_str().unwrap_or(id).into(), plan.clone(), Apply::Quality(plan))
}

pub(in crate::render_workspace) fn light(app: &mut SolveApp, view: &View, id: &str) -> Result<(), String> {
    if let Some(reason) = light_reason(app, &view.kind) {
        return Err(reason);
    }
    let set = patch(app)?;
    let name = app.render_workspace.view.name.clone();
    let label = view.details.as_ref().and_then(|d| d["resource"]["name"].as_str()).unwrap_or(id).to_string();
    match view.kind.as_str() {
        "native_scenes" => {
            let mut params = json!({"id":id,"name":name,"set":set,"expected_revision":app.session.revision});
            let plan = app.run("render.preset.resolve", params.clone())?;
            params["replace_lighting_and_environment"] = json!(true);
            params["replace_stage"] = json!(true);
            open(app, "studio", label, plan, Apply::Command { id: "render.preset.apply".into(), params })
        }
        "hdr" => open(
            app,
            "environment",
            label,
            view.details.clone().unwrap_or_default(),
            Apply::Command {
                id: "render.library.environment.apply".into(),
                params: json!({"id":id,"name":name,"set":set,"replace_environment":true}),
            },
        ),
        "rigs" => {
            let rig = json!({"id":id,"variant":"complete","center_mm":view.center,"subject_extent_mm":view.extent});
            let plan = app.run("render.library.studio.prepare", rig.clone())?;
            open(
                app,
                "rig",
                label,
                plan,
                Apply::Command {
                    id: "render.library.studio.apply".into(),
                    params: json!({"rig":rig,"name":name,"set":set,"replace_lighting_and_environment":true,"replace_stage":true}),
                },
            )?;
            if let Some(draft) = &mut app.render_workspace.view.library.review {
                draft.center = view.center;
                draft.extent = view.extent;
            }
            Ok(())
        }
        _ => Err(app.tr("render.library_specification_only").into()),
    }
}

fn apply(app: &mut SolveApp, draft: &mut Draft) -> Result<(), String> {
    if !draft.current(app) || !draft.acknowledged {
        return Err(app.tr("render.review_stale").into());
    }
    match &mut draft.action {
        Apply::Command { id, params } => {
            if draft.variant == "rig" {
                params["rig"]["center_mm"] = json!(draft.center);
                params["rig"]["subject_extent_mm"] = json!(draft.extent);
                params["rig"]["variant"] = json!(if draft.emitters_only { "emitters_only" } else { "complete" });
                app.run("render.library.studio.prepare", params["rig"].clone())?;
            }
            app.run(id, params.clone())?;
            if id.as_str() != "render.library.appearance.apply" {
                setups::load(app);
            }
        }
        Apply::Maps { id, target, plane, scale } => materials::install_library_surface(app, id, target.clone(), plane, *scale)?,
        Apply::Quality(plan) => {
            let before = setups::draft(app)?;
            super::super::inspector::apply_quality(app, plan)?;
            setups::persist_editor(app, &before)?;
        }
        Apply::Physical(_) => {}
    }
    Ok(())
}

pub(in crate::render_workspace) fn dialog(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut draft) = app.render_workspace.view.library.review.take() else { return };
    let current = draft.current(app);
    let title = app.tr(&format!("render.stage_panels.review_{}", draft.variant)).to_string();
    let subtitle = if draft.variant == "rig" {
        let emitters = draft.plan["lighting"]["area_lights"].as_array().map_or(0, Vec::len) + usize::from(!draft.plan["lighting"]["sun"].is_null());
        let cards = draft.plan["stage"].as_array().map_or(0, Vec::len);
        crate::i18n::format("render.stage_panels.rig_summary", &[("emitters", &emitters.to_string()), ("cards", &cards.to_string())])
    } else {
        app.tr(&format!("render.stage_panels.subtitle_{}", draft.variant)).to_string()
    };
    let replaces = app.tr(&format!("render.stage_panels.replaces_{}", draft.variant)).to_string();
    let kept = app.tr(if draft.variant == "quality" { "render.stage_panels.kept_quality" } else { "render.stage_panels.kept_fields" }).to_string();
    let acknowledgement = app
        .tr(match draft.variant {
            "studio" => "render.stage_panels.studio_ack",
            "rig" => "render.library_replace_studio",
            "physical" => "render.physical_acknowledge",
            "quality" => "render.stage_panels.quality_ack",
            "stage" => "render.replace_ground",
            _ => "render.stage_panels.acknowledge",
        })
        .to_string();
    let apply_label = app
        .tr(match draft.variant {
            "studio" => "render.stage_panels.apply_studio",
            "rig" => "render.stage_panels.fit_save",
            "physical" => "ui.close",
            "quality" => "render.stage_panels.use_budget",
            _ => "render.stage_panels.apply",
        })
        .to_string();
    let physical_notice = app.tr("render.physical_plan_only").to_string();
    let review = ui_kit::ReviewAndApply {
        title: &title,
        name: &draft.name,
        subtitle: &subtitle,
        replaces: &replaces,
        kept: &kept,
        acknowledgement: &acknowledgement,
        apply: &apply_label,
        copy: draft.variant == "physical",
        notice: (draft.variant == "physical").then_some(physical_notice.as_str()),
    };
    let (closed, answer) = review.show(ctx, current, &mut draft.acknowledged, |ui| {
        if !current {
            ui_kit::text(ui, app.tr("render.review_stale"), ui_kit::Text::Error("review:stale"));
        }
        match draft.variant {
            "rig" => {
                let mut changed = false;
                ui_kit::row(ui, app.tr("render.stage_panels.centre"), |ui| {
                    ui.horizontal(|ui| {
                        for (axis, coordinate) in draft.center.iter_mut().enumerate() {
                            let field = ui_kit::number_field(ui, coordinate, -1e12..=1e12, 1.0, " mm");
                            changed |= field.changed();
                            crate::scenario::publish_target(&format!("render:review:center:{axis}"), field.rect);
                        }
                    });
                });
                ui_kit::row(ui, app.tr("render.stage_panels.extent"), |ui| {
                    let field = ui_kit::number_field(ui, &mut draft.extent, 0.001..=1e6, 1.0, " mm");
                    changed |= field.changed();
                    crate::scenario::publish_target("render:review:extent", field.rect);
                });
                let bounds = ui_kit::checkbox(ui, app.tr("render.stage_panels.captured_bounds"), &mut draft.bounds);
                crate::scenario::publish_target("render:review:bounds", bounds.rect);
                if bounds.changed() && draft.bounds {
                    let mut targets = View::default();
                    if let Err(error) = refresh_targets(app, &mut targets) {
                        app.set_status(error, true);
                    }
                    draft.center = targets.center;
                    draft.extent = targets.extent;
                }
                if changed {
                    draft.bounds = false;
                }
                let omit = ui_kit::checkbox(ui, app.tr("render.library_emitters_only_ack"), &mut draft.emitters_only);
                crate::scenario::publish_target("render:review:emitters-only", omit.rect);
            }
            "quality" => {
                ui_kit::text(ui, app.tr("render.quality_projection_help"), ui_kit::Text::Note("review:quality"));
                ui_kit::row(ui, app.tr("render.stage_panels.projects"), |ui| {
                    ui_kit::text(
                        ui,
                        &format!(
                            "{} × {} · {} spp · {} {}",
                            draft.plan["settings"]["width"],
                            draft.plan["settings"]["height"],
                            draft.plan["gpu_options"]["samples"],
                            app.tr("render.stage_panels.depth"),
                            draft.plan["gpu_options"]["max_depth"]
                        ),
                        ui_kit::Text::Inline,
                    );
                });
                ui_kit::text(ui, app.tr("render.stage_panels.omits"), ui_kit::Text::Note("review:omits"));
                ui_kit::text(ui, app.tr("render.stage_panels.quality_omissions"), ui_kit::Text::Note("review:omissions"));
            }
            "physical" => {
                ui_kit::row(ui, app.tr("render.stage_panels.reference"), |ui| {
                    ui_kit::text(ui, &draft.name, ui_kit::Text::Inline);
                });
                let count = draft.plan["extended_profile"]["properties"].as_array().map_or(0, Vec::len);
                let candidates = crate::i18n::format("render.stage_panels.candidate_summary", &[("count", &count.to_string())]);
                ui_kit::text(ui, &candidates, ui_kit::Text::Note("review:candidates"));
                ui_kit::text(ui, app.tr("render.stage_panels.partial_reference"), ui_kit::Text::Note("review:partial"));
            }
            _ => {}
        }
    });
    if answer == ui_kit::Action::Apply
        && let Apply::Physical(plan) = &draft.action
    {
        ctx.copy_text(serde_json::to_string_pretty(plan).unwrap_or_default());
    }
    if answer == ui_kit::Action::Ok {
        if let Err(error) = apply(app, &mut draft) {
            app.set_status(error, true);
            draft.acknowledged = false;
            app.render_workspace.view.library.review = Some(draft);
        }
    } else if !closed && answer != ui_kit::Action::Cancel {
        app.render_workspace.view.library.review = Some(draft);
    }
}

#[derive(Clone, Debug)]
pub(in crate::render_workspace) struct Removal {
    command: String,
    params: Value,
    revision: u64,
    document: Weak<Document>,
}
#[cfg(test)]
pub(in crate::render_workspace) fn remove(app: &mut SolveApp, command: &str, params: Value) {
    app.render_workspace.view.library.removal =
        Some(Removal { command: command.into(), params, revision: app.session.revision, document: Arc::downgrade(&app.session.doc) });
}
pub(in crate::render_workspace) fn confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending) = app.render_workspace.view.library.removal.take() else { return };
    if pending.revision != app.session.revision || !Weak::ptr_eq(&pending.document, &Arc::downgrade(&app.session.doc)) {
        return;
    }
    let stage = pending.command == "render.stage.clear";
    let title = app.tr(if stage { "render.stage_panels.remove_stage_title" } else { "render.stage_panels.remove_optics_title" }).to_string();
    let message = app.tr(if stage { "render.stage_panels.remove_stage_help" } else { "render.stage_panels.remove_optics_help" }).to_string();
    let label = app.tr(if stage { "render.stage_panels.remove_stage" } else { "render.stage_panels.remove_optics" }).to_string();
    let (closed, answer) = ui_kit::Confirm::new(egui::Id::new("render_resource_removal"), "delete", &title)
        .tone(ui_kit::Tone::Danger)
        .actions(&label, app.tr("render.stage_panels.keep"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &message, ui_kit::Text::Note("remove:message"));
        });
    if answer == ui_kit::Action::Ok {
        match app.run(&pending.command, pending.params) {
            Ok(_) => setups::load(app),
            Err(error) => app.set_status(error, true),
        }
    } else if !closed && answer != ui_kit::Action::Cancel {
        app.render_workspace.view.library.removal = Some(pending);
    }
}

#[cfg(test)]
pub(in crate::render_workspace) fn command(app: &mut SolveApp, id: &str, params: Value) {
    let result = open(app, "stage", app.tr("render.passive_stage").into(), Value::Null, Apply::Command { id: id.into(), params });
    if let Err(error) = result {
        app.set_status(error, true);
    }
}
