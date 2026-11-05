//! Result comparison composition. The kit owns controls, chips and the divider.
use super::*;
use crate::ui_kit;
use solidcraft_engine::render::job::JobId;

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct View {
    pub left: Option<JobId>,
    pub right: Option<JobId>,
    pub position: f32,
    pub reference: JobId,
    #[serde(skip)]
    pub dragging: bool,
}

fn side_value(app: &SolveApp, side: Option<JobId>) -> Option<serde_json::Value> {
    match side {
        None => Some(json!("live")),
        Some(job) => app.render_workspace.view.retained.iter().position(|result| result.frame.job == job).map(|index| json!(index)),
    }
}

pub(super) fn apply(app: &mut SolveApp, reply: &serde_json::Value) -> Result<(), String> {
    if reply.get("off").and_then(serde_json::Value::as_bool) == Some(true) {
        app.render_workspace.view.compare = None;
        app.render_workspace.comparison_images.clear();
        return Ok(());
    }
    sync_results(app);
    let [left, right] = app.session.render_comparison().ok_or("comparison result is no longer retained")?;
    let position = app.render_workspace.view.compare.as_ref().map_or(0.5, |view| view.position);
    let reference = app
        .render_workspace
        .view
        .compare
        .as_ref()
        .map(|view| view.reference)
        .filter(|job| [left, right].contains(&Some(*job)))
        .or(right)
        .or(left)
        .ok_or("comparison needs a retained result")?;
    if app.render_workspace.view.compare.as_ref().is_none_or(|view| view.reference != reference) {
        app.render_workspace.view.result_scale = ResultScale::Fit;
    }
    app.render_workspace.view.compare = Some(View { left, right, position, reference, dragging: false });
    app.render_workspace.view.result = true;
    app.render_workspace.view.follow_job = false;
    app.render_workspace.preview_image = None;
    app.cancel_view_animation();
    app.viewport.nav = None;
    app.viewport.key_hold = None;
    app.viewport.orbit_free = None;
    Ok(())
}

pub(super) fn start_default(app: &mut SolveApp) -> Result<(), String> {
    sync_results(app);
    if app.render_workspace.view.compare.is_some() {
        app.run("render.compare", json!({"off":true}))?;
    } else {
        let index = app
            .render_workspace
            .view
            .retained
            .iter()
            .position(|result| Some(result.frame.job) == app.render_workspace.view.completed)
            .ok_or_else(|| app.tr("render.ribbon_no_result").to_string())?;
        app.run("render.compare", json!({"left":"live","right":index}))?;
    }
    Ok(())
}

pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let results = &app.render_workspace.view.retained;
    if results.is_empty() {
        return;
    }
    let mut labels = vec![app.tr("render.compare_live").to_owned()];
    labels.extend(results.iter().map(|result| crate::i18n::format("render.compare_render", &[("number", &result.frame.job.serial.to_string())])));
    let mut targets = vec!["render:compare:live".to_string()];
    targets.extend(results.iter().enumerate().map(|(index, _)| format!("render:compare:result:{index}")));
    let choices: Vec<_> = std::iter::once(None).chain(results.iter().map(|result| Some(result.frame.job))).collect();
    let current = if app.render_workspace.view.result { app.render_workspace.view.completed } else { None };
    let comparison = app.render_workspace.view.compare.clone();
    let states: Vec<_> =
        choices.iter().map(|side| comparison.as_ref().map_or(*side == current, |view| *side == view.left || *side == view.right)).collect();
    let mut selected = usize::MAX;
    if ui_kit::segmented(
        ui,
        &mut selected,
        &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        ui_kit::Segments {
            targets: &targets.iter().map(String::as_str).collect::<Vec<_>>(),
            states: &states,
            intrinsic: true,
            dense: true,
            compact: true,
            workspace_accent: true,
            ..Default::default()
        },
    ) && let Some(chosen) = choices.get(selected).copied()
    {
        if let Some(view) = comparison {
            if chosen == view.left || chosen == view.right {
                let remaining = if chosen == view.left { view.right } else { view.left };
                let _ = app.run("render.compare", json!({"off":true}));
                choose_single(app, remaining);
            } else if let (Some(left), Some(right)) = (side_value(app, view.left), side_value(app, chosen)) {
                let _ = app.run("render.compare", json!({"left":left,"right":right}));
            }
        } else if chosen != current {
            let (left, right) = if chosen.is_none() { (chosen, current) } else { (current, chosen) };
            if let (Some(left), Some(right)) = (side_value(app, left), side_value(app, right)) {
                let _ = app.run("render.compare", json!({"left":left,"right":right}));
            }
        }
    }
    if let Some(view) = app.render_workspace.view.compare.clone() {
        let swap = ui_kit::compact_icon_label_button(ui, app.tr("render.compare_swap"), "compare", true);
        crate::scenario::publish_target("render:compare:swap", swap.rect);
        if swap.clicked()
            && let (Some(left), Some(right)) = (side_value(app, view.right), side_value(app, view.left))
        {
            let _ = app.run("render.compare", json!({"left":left,"right":right}));
        }
    }
}

fn choose_single(app: &mut SolveApp, side: Option<JobId>) {
    match side {
        None => {
            let _ = app.run("render.result.view", json!({"view":"model"}));
        }
        Some(job) => {
            if let Some(index) = app.render_workspace.view.retained.iter().position(|result| result.frame.job == job) {
                let _ = app.run("render.result.select", json!({"index":index}));
            }
        }
    }
}

fn texture(app: &mut SolveApp, ctx: &egui::Context, job: JobId) -> Result<egui::TextureHandle, String> {
    if let Some((_, image)) = app.render_workspace.comparison_images.iter().find(|(id, _)| *id == job) {
        return image.clone();
    }
    if let Some((id, image)) = &app.render_workspace.image
        && *id == job
    {
        let image = image.clone();
        app.render_workspace.comparison_images.push((job, image.clone()));
        return image;
    }
    let image: Result<egui::TextureHandle, String> = (|| {
        let handle = app.session.render_job_handle(job).map_err(|error| error.to_string())?;
        let frame = handle.frame().map_err(|error| error.to_string())?.ok_or("comparison image is not completed")?;
        let captured = handle.status().map_err(|error| error.to_string())?;
        let limits = if frame.info.executor.as_str() == "gpu-path" {
            solidcraft_engine::render::job::GPU_PATH_LIMITS
        } else {
            captured_image_limits(&captured.request)
        };
        let image = decode_render_image(frame.png(), frame.info.width, frame.info.height, limits, ctx.input(|input| input.max_texture_side))?;
        Ok(ctx.load_texture("sc_render_compare", image, egui::TextureOptions::LINEAR))
    })();
    app.render_workspace.comparison_images.push((job, image.clone()));
    image
}

fn label(app: &SolveApp, side: Option<JobId>) -> String {
    side.and_then(|job| app.render_workspace.view.retained.iter().find(|result| result.frame.job == job)).map_or_else(
        || app.tr("render.compare_live_preview").to_owned(),
        |result| {
            crate::i18n::format(
                "render.compare_side",
                &[
                    ("number", &result.frame.job.serial.to_string()),
                    ("shot", result.shot.as_deref().unwrap_or_else(|| crate::i18n::tr("render.results_unnamed"))),
                ],
            )
        },
    )
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    cache_render_images(app, ui.ctx());
    if app.render_workspace.view.compare.is_none() {
        show_result(app, ui);
        return;
    }
    ui.painter().rect_filled(ui.max_rect(), 0.0, Tokens::get().viewport_bottom);
    let mut free = crate::render_ui::canvas(app, ui.max_rect());
    if ribbon::busy(app) {
        free = ui_kit::Container::below_navigation_row(free);
    }
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(free).id_salt("render_compare_content"));
    let ui = &mut content;
    ui_kit::Container::navigation_contents(ui, |ui| {
        ui.horizontal_wrapped(|ui| result_actions(app, ui));
    });
    let Some(view) = app.render_workspace.view.compare.clone() else { return };
    app.render_workspace.comparison_images.retain(|(job, _)| [view.left, view.right].contains(&Some(*job)));
    let Some(reference) = app.render_workspace.view.retained.iter().find(|result| result.frame.job == view.reference).cloned() else { return };
    let stale = app.render_workspace.view.retained.iter().any(|result| [view.left, view.right].contains(&Some(result.frame.job)) && result.stale);
    // Keep freshness and the capture summary below the image, as in the result board.
    ui_kit::Panel::pinned_footer(ui, "render_compare_summary", 2, |ui| {
        ui_kit::completion_text(ui, app.tr(if stale { "render.image_stale" } else { "render.result_current" }), !stale);
        let summary = crate::i18n::format(
            "render.compare_summary",
            &[
                ("shot", reference.shot.as_deref().unwrap_or_else(|| crate::i18n::tr("render.results_unnamed"))),
                ("width", &reference.frame.width.to_string()),
                ("height", &reference.frame.height.to_string()),
                ("seconds", &format!("{:.0}", reference.seconds)),
                ("renderer", reference.frame.executor.as_str()),
                ("revision", &reference.frame.scene_generation.to_string()),
            ],
        );
        ui_kit::text(ui, &summary, ui_kit::Text::Inline);
    });
    let canvas = ui.available_rect_before_wrap();
    if !canvas.is_positive() {
        return;
    }
    let response = ui.allocate_rect(canvas, Sense::click_and_drag());
    app.viewport.rect = Some(canvas);
    crate::viewport::refresh_layout(app, ui.ctx());
    let size = [reference.frame.width, reference.frame.height];
    if app.render_workspace.view.result_scale != ResultScale::Free {
        app.render_workspace.view.result_camera = result_camera(size, canvas, ui.ctx().pixels_per_point(), app.render_workspace.view.result_scale);
    }
    // Reserve the divider before navigation; captured drags cannot reach any camera path.
    let mut position = view.position;
    let divider = ui_kit::comparison_divider(ui, canvas, &mut position, app.tr("render.compare_divider"));
    let captured = divider.dragged() || divider.is_pointer_button_down_on() || divider.drag_stopped();
    if let Some(view) = &mut app.render_workspace.view.compare {
        view.dragging = divider.is_pointer_button_down_on() || divider.dragged();
    }
    if divider.changed()
        && let Err(error) = app.run("render.compare.divider", json!({"position":position}))
    {
        app.set_status(error, true);
    }
    if !captured {
        with_result_camera(app, |app| {
            crate::navigation::handle(app, ui, canvas, &response, crate::navigation::NavCaps { orbit: false, pick: false });
        });
    }
    let image = result_rect(app.render_workspace.view.result_camera, size, canvas);
    let split = canvas.left() + canvas.width() * position;
    let halves = [
        egui::Rect::from_min_max(canvas.min, egui::pos2(split, canvas.bottom())),
        egui::Rect::from_min_max(egui::pos2(split, canvas.top()), canvas.max),
    ];
    let camera = app.session.render_job_handle(reference.frame.job).ok().and_then(|handle| handle.status().ok()).map(|status| status.request.camera);
    // Paint sources behind the already registered handle, then raise its shared chrome.
    for ((side, half), right) in [view.left, view.right].into_iter().zip(halves).zip([false, true]) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(canvas).id_salt(("compare_side", right)));
        child.set_clip_rect(half.intersect(image).intersect(ui.clip_rect()));
        if child.clip_rect().is_positive() {
            match side {
                None => {
                    if let Some(camera) = camera {
                        paint_product(app, &mut child, canvas, camera, Some(image));
                    }
                }
                Some(job) => match texture(app, ui.ctx(), job) {
                    Ok(texture) => {
                        let source = app.render_workspace.view.retained.iter().find(|result| result.frame.job == job);
                        let aspect = source.map_or(1.0, |result| result.frame.width as f32 / result.frame.height.max(1) as f32);
                        let height = image.height().min(image.width() / aspect);
                        let destination = egui::Rect::from_center_size(image.center(), egui::vec2(height * aspect, height));
                        child.painter().image(
                            texture.id(),
                            destination,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            Tokens::get().file_image_tint,
                        );
                    }
                    Err(error) => {
                        ui_kit::text(&mut child, &error, ui_kit::Text::Inline);
                    }
                },
            }
        }
        ui_kit::comparison_badge(ui, half, &label(app, side), right);
    }
    // The line and handle must stay above both source callbacks and label chips.
    ui_kit::comparison_divider_chrome(ui, canvas, position);
    crate::scenario::publish_rect("render:compare:canvas", canvas);
    crate::scenario::publish_rect("render:compare:image", image);
}
