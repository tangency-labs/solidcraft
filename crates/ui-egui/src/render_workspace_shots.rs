//! Shot strip adapters. Commands own saved intent; the kit owns every control and layout.
use crate::{SolveApp, ui_kit};
use serde_json::json;
use solidcraft_engine::{doc::render_settings::RenderSettings, render::job::Phase};

pub(super) const SIZE_PRESETS: [(&str, u32, u32); 3] = [("HD", 1280, 720), ("Full HD", 1920, 1080), ("4K UHD", 3840, 2160)];

pub(super) fn camera_modified(app: &SolveApp, name: &str) -> bool {
    app.session.doc.render.setups.get(name).is_some_and(|setup| setup.camera != app.cam)
}

/// Update just the saved camera, preserving all the shot's other saved intent.
pub(super) fn update_camera(app: &mut SolveApp) -> Result<(), String> {
    let name = app.render_workspace.view.name.clone();
    app.run("render.settings.set", json!({"name":name,"set":{"camera":app.cam}}))?;
    Ok(())
}

pub(crate) fn contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    super::thumbnails::refresh(app, ui.ctx());
    let status = app.render_workspace.view.job.and_then(|job| app.session.render_job_handle(job).ok()).and_then(|handle| handle.status().ok());
    let progress = status
        .as_ref()
        .filter(|s| matches!(s.progress.phase, Phase::Queued | Phase::Preparing | Phase::Running | Phase::Encoding | Phase::Cancelling))
        .map(|s| (s.progress.done as f64 / s.progress.total.max(1) as f64).clamp(0.0, 1.0) as f32);
    if progress.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    let ready = super::job_ready(app, "render.start");
    let draft = &app.render_workspace.view.draft;
    let size_label = format!("{} × {}", draft.width, draft.height);
    let completed = app
        .render_workspace
        .view
        .completed
        .and_then(|job| app.session.render_job_handle(job).ok())
        .and_then(|handle| handle.frame().ok().flatten())
        .map(|frame| frame.info.clone());
    let result_label = completed
        .as_ref()
        .map(|frame| {
            crate::i18n::format(
                "render.shots_result",
                &[("number", &frame.job.serial.to_string()), ("width", &frame.width.to_string()), ("height", &frame.height.to_string())],
            )
        })
        .unwrap_or_else(|| app.tr("render.shots_no_result").to_string());
    let (mut hide, mut start, mut save, mut chosen, mut size) = (false, false, false, None, None);
    let mut visible = Vec::new();
    ui_kit::Panel::strip_contents(
        ui,
        |ui| {
            let response = ui_kit::strip_toggle(ui, true, crate::i18n::tr_label("Hide Shots strip"));
            crate::scenario::publish_target("render:shots:hide", response.rect);
            hide = response.clicked();
            let response = ui_kit::icon_label_action(ui, app.tr("cmd.render_start"), Some("play"), ready.is_ok(), true);
            let response = if let Err(reason) = &ready { response.on_disabled_hover_text(reason) } else { response };
            crate::scenario::publish_target("render:shots:render", response.rect);
            start = response.clicked();
            let response = ui_kit::compact_dropdown_menu(ui, "render_shots_size", &size_label, |ui| {
                for (label, width, height) in SIZE_PRESETS {
                    if ui_kit::list_picker(ui, label, &format!("{label} · {width} × {height}"), draft.width == width && draft.height == height)
                        .clicked()
                    {
                        size = Some((width, height));
                        ui.close();
                    }
                }
            });
            crate::scenario::publish_target("render:shots:size", response.rect);
            let response = ui_kit::completion_text(ui, &result_label, completed.is_some());
            crate::scenario::publish_rect("render:shots:status", response.rect);
        },
        |ui| {
            let heading = ui_kit::StripHeading::new(ui.painter(), crate::i18n::tr_label("Shots"));
            let response = heading.show(ui);
            crate::scenario::publish_rect("render:shots:heading", response.rect);
            let response = ui.add_enabled_ui(super::setups::snapshot(app).is_ok(), |ui| ui_kit::add_choice(ui, app.tr("render.shots_save"))).inner;
            crate::scenario::publish_target("render:shots:save", response.rect);
            save = response.clicked();
            for name in app.session.doc.render.setups.keys() {
                let active = name == &app.render_workspace.view.name;
                let response = ui_kit::thumbnail_choice(
                    ui,
                    &format!("shot:{name}"),
                    name,
                    ui_kit::ThumbnailChoice {
                        texture: app.render_workspace.thumbnails.texture(name),
                        selected: active,
                        modified: active && camera_modified(app, name),
                        progress: if app.render_workspace.view.job_shot.as_ref() == Some(name) { progress } else { None },
                    },
                );
                crate::scenario::publish_target(&format!("render:shots:setup:{name}"), response.rect);
                if visible.len() < super::thumbnails::LIMIT && response.rect.intersects(ui.clip_rect()) {
                    visible.push(name.clone());
                }
                if response.clicked() {
                    chosen = Some(name.clone());
                }
            }
        },
    );
    app.render_workspace.thumbnails.show(visible, ui.ctx());
    if hide {
        let _ = app.run("view.shots_strip", json!({"visible":false}));
    }
    if start {
        app.start("render.start");
    }
    if save {
        app.start("render.settings.set");
    }
    if let Some(name) = chosen {
        super::switch_shot(app, name);
    }
    if let Some((width, height)) = size {
        app.render_workspace.view.draft.width = width;
        app.render_workspace.view.draft.height = height;
        app.render_workspace.view.dirty = true;
    }
}

pub(super) fn confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut name) = app.render_workspace.view.save_shot_name.take() else { return };
    let valid = RenderSettings::validate_name(name.trim()).is_ok() && !app.session.doc.render.setups.contains_key(name.trim());
    let title = app.tr("render.shots_save").to_string();
    let cancel = app.tr("ui.cancel").to_string();
    let (closed, answer) =
        ui_kit::Confirm::new(egui::Id::new("render_save_shot"), "save", &title).actions(&title, &cancel, None).show(ctx, valid, |ui| {
            ui_kit::text(ui, app.tr("render.shots_name_help"), ui_kit::Text::Note("render.shots_name_help"));
            ui_kit::row(ui, app.tr("render.shot_new_name"), |ui| {
                ui_kit::focused_text_field(ui, &mut name, "");
            });
        });
    if closed || answer == ui_kit::Action::Cancel {
        return;
    }
    if answer == ui_kit::Action::Ok {
        let previous = app.render_workspace.view.name.clone();
        app.render_workspace.view.name = name.trim().to_owned();
        if let Err(error) = super::setups::save(app) {
            app.render_workspace.view.name = previous;
            app.set_status(error, true);
        } else {
            return;
        }
    }
    app.render_workspace.view.save_shot_name = Some(name);
}
