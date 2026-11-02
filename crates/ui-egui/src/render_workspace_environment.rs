//! Standalone Environment inspector body. Drawing and source browsing never save a setup.
use super::{ImageMethod, Stage, cpu, hdr, setups, studio};
use crate::{SolveApp, theme::Tokens, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::doc::render_settings::{RenderJob, RenderSetup};
use std::sync::{Arc, Weak};

#[derive(Clone, Debug, Default)]
pub(super) struct RingView {
    pub geometry: Option<Ring>,
    drag: Option<RingDrag>,
    suppress_primary: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Ring {
    pub center: egui::Pos2,
    pub radius: f32,
}

#[derive(Clone, Debug)]
struct RingDrag {
    ring: Ring,
    viewport: egui::Rect,
    document: Weak<solidcraft_engine::doc::Document>,
    revision: u64,
    name: String,
    start: f64,
    initial: f64,
    current: f64,
    moved: bool,
}

/// Clockwise screen-space angle. An undefined centre or collapsed ring is not a gesture.
pub(super) fn pointer_angle(pointer: egui::Pos2, center: egui::Pos2, radius: f32) -> Option<f64> {
    if !pointer.is_finite() || !center.is_finite() || !radius.is_finite() || !(1.0..=1e6).contains(&radius) {
        return None;
    }
    let x = f64::from(pointer.x) - f64::from(center.x);
    let y = f64::from(pointer.y) - f64::from(center.y);
    (x.hypot(y) >= 1e-6).then(|| y.atan2(x).to_degrees())
}

pub(super) fn drag_rotation(initial: f64, start: f64, angle: f64, snap: bool) -> Option<f64> {
    if [initial, start, angle].iter().any(|value| !value.is_finite() || value.abs() > 1e6) {
        return None;
    }
    let delta = (angle - start + 180.0).rem_euclid(360.0) - 180.0;
    let value = initial + delta;
    let value = if snap { (value / 15.0).round() * 15.0 } else { value };
    Some((value + 180.0).rem_euclid(360.0) - 180.0)
}

fn ring_target(app: &SolveApp) -> bool {
    let view = &app.render_workspace.view;
    super::active(app)
        && !view.result
        && view.selection == super::RenderSelection::Environment
        && environment_values(app).is_some_and(|(_, _, _, rotates)| rotates)
        && !app.session.model.is_stale()
}

fn ring_geometry(app: &SolveApp, viewport: egui::Rect) -> Option<Ring> {
    let free = crate::render_ui::canvas(app, viewport);
    if !viewport.is_finite() || !viewport.is_positive() || !free.is_finite() || !free.is_positive() {
        return None;
    }
    let world = crate::open_meshing::shown_world(app);
    let bounds = crate::viewport::body_bounds(app, &world)
        .iter()
        .filter(|(name, _)| app.session.body_visible(name))
        .fold(solidcraft_engine::geom::Aabb3::EMPTY, |a, (_, b)| a.union(b));
    if bounds.is_empty() {
        return None;
    }
    let subject = crate::viewport::screen_box(&crate::viewport::projection(app, viewport), &bounds, 0.0)?;
    let center = subject.center();
    if !subject.is_finite() || !center.is_finite() || !free.contains(center) {
        return None;
    }
    let t = Tokens::get();
    let maximum = (center.x - free.left()).min(free.right() - center.x).min(center.y - free.top()).min(free.bottom() - center.y) - t.space_12;
    let minimum = t.control_height * 2.0;
    if !maximum.is_finite() || maximum < minimum {
        return None;
    }
    let radius = (subject.size().length() * 0.5 + t.space_24).clamp(minimum, maximum.min(1e6));
    Some(Ring { center, radius })
}

/// Own the complete primary gesture before shared camera navigation or product picking sees it.
pub(super) fn ring_input(app: &mut SolveApp, ui: &mut egui::Ui, viewport: egui::Rect, response: &egui::Response) -> bool {
    if app.render_workspace.view.environment_ring.suppress_primary {
        app.render_workspace.view.environment_ring.suppress_primary = ui.input(|i| i.pointer.primary_down());
        return true;
    }
    let mut drag = app.render_workspace.view.environment_ring.drag.take();
    let captured = drag.is_some();
    let valid = ring_target(app) && setups::snapshot(app).is_ok();
    let fresh = drag.as_ref().is_none_or(|d| {
        d.revision == app.session.revision
            && d.name == app.render_workspace.view.name
            && d.viewport == viewport
            && Weak::ptr_eq(&d.document, &Arc::downgrade(&app.session.doc))
            && environment_values(app).is_some_and(|(rotation, _, _, _)| rotation == d.initial)
    });
    if !valid || !fresh {
        app.render_workspace.view.environment_ring.geometry = None;
        app.render_workspace.view.environment_ring.suppress_primary = captured && ui.input(|i| i.pointer.primary_down());
        return captured;
    }
    let geometry = drag.as_ref().map(|d| d.ring).or_else(|| ring_geometry(app, viewport));
    app.render_workspace.view.environment_ring.geometry = geometry;
    let Some(ring) = geometry else { return captured };
    let (pointer, origin, pressed, down, released, shift, escape) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.press_origin(),
            i.pointer.button_pressed(egui::PointerButton::Primary),
            i.pointer.primary_down(),
            i.pointer.button_released(egui::PointerButton::Primary),
            i.modifiers.shift,
            i.key_pressed(egui::Key::Escape),
        )
    });
    let hit = |p: egui::Pos2| p.is_finite() && (p.distance(ring.center) - ring.radius).abs() <= Tokens::get().space_8;
    if drag.is_none()
        && pressed
        && ui.is_enabled()
        && response.contains_pointer()
        && !egui::Popup::is_any_open(ui.ctx())
        && app.dialog.is_none()
        && let Some(origin) = origin.filter(|p| !crate::shell::covered(app, *p) && hit(*p))
        && let Some(start) = pointer_angle(origin, ring.center, ring.radius)
        && let Some((initial, _, _, _)) = environment_values(app)
    {
        drag = Some(RingDrag {
            ring,
            viewport,
            document: Arc::downgrade(&app.session.doc),
            revision: app.session.revision,
            name: app.render_workspace.view.name.clone(),
            start,
            initial,
            current: initial,
            moved: false,
        });
    }
    if let Some(mut drag) = drag {
        app.begin_external_navigation();
        app.viewport.orbit_from = None;
        app.viewport.primary_nav = None;
        app.viewport.modifier_nav = None;
        app.viewport.key_hold = None;
        app.viewport.device_motion = None;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let angle = pointer.and_then(|p| pointer_angle(p, ring.center, ring.radius)).and_then(|angle| {
            drag.moved |= drag_rotation(0.0, drag.start, angle, false).is_some_and(|delta| delta.abs() > 1e-6);
            drag_rotation(drag.initial, drag.start, angle, shift && drag.moved)
        });
        if escape || angle.is_none() || (!down && !released) {
            app.render_workspace.view.environment_ring.suppress_primary = down;
            return true;
        }
        if let Some(angle) = angle {
            drag.current = angle;
        }
        if released {
            if drag.moved
                && (drag.current - drag.initial).abs() > 1e-9
                && let Err(error) = commit_environment(app, json!({"rotation_deg":drag.current}))
            {
                app.set_status(error, true);
            }
        } else {
            app.render_workspace.view.environment_ring.drag = Some(drag);
        }
        ui.ctx().request_repaint();
        return true;
    }
    if ui.is_enabled() && pointer.is_some_and(hit) && response.contains_pointer() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    false
}

pub(super) fn ring_overlay(app: &mut SolveApp, ui: &mut egui::Ui, viewport: egui::Rect) {
    if !ring_target(app) {
        return;
    }
    let view = &app.render_workspace.view.environment_ring;
    let Some(ring) = view.geometry else { return };
    let angle = view.drag.as_ref().map(|drag| drag.current).or_else(|| environment_values(app).map(|v| v.0)).unwrap_or(0.0);
    let t = Tokens::get();
    let free = crate::render_ui::canvas(app, viewport);
    let painter = ui.painter().with_clip_rect(free.intersect(viewport));
    painter.circle_stroke(ring.center, ring.radius, egui::Stroke::new(t.space_2, t.form_primary));
    let radians = angle.to_radians();
    let handle = ring.center + egui::vec2(radians.cos() as f32, radians.sin() as f32) * ring.radius;
    painter.circle_filled(handle, t.space_6, t.form_primary);
    painter.circle_stroke(handle, t.space_6, t.control_border_stroke(t.on_accent));
    crate::scenario::publish_target("render:environment:ring", egui::Rect::from_center_size(ring.center, egui::Vec2::splat(ring.radius * 2.0)));
    crate::scenario::publish_target("render:environment:ring-handle", egui::Rect::from_center_size(handle, egui::Vec2::splat(t.space_16)));
    let chip = ui_kit::Container::navigation_card(
        ui.ctx(),
        egui::Id::new("render_environment_rotation"),
        ring.center + egui::vec2(0.0, ring.radius - t.control_height - t.space_8),
        egui::Align2::CENTER_BOTTOM,
        |ui| ui_kit::text(ui, &format!("{angle:.0}°"), ui_kit::Text::Inline).on_hover_text(app.tr("render.env_ring_help")),
    );
    crate::scenario::publish_target("render:environment:ring-degrees", chip.response.rect);
}

pub(super) fn editor(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::scope(ui, |ui| {
        source(app, ui);
        let title = app.tr("render.environment.orientation").to_string();
        ui_kit::section(ui, "render_environment_orientation", &title, true, |ui| {
            let snapshot = setups::snapshot(app);
            let controls = environment_values(app);
            let valid = snapshot.is_ok() && controls.is_some();
            let (rotation, intensity, maximum, rotates) = controls.unwrap_or((0.0, 1.0, 100.0, false));
            ui.add_enabled_ui(valid, |ui| {
                let mut request = None;
                ui_kit::row(ui, app.tr("render.env_rotate"), |ui| {
                    ui.add_enabled_ui(rotates, |ui| {
                        if let Some(value) = edit(app, ui, "rotation-slider", rotation, |ui, value| {
                            let response =
                                ui_kit::slider(ui, value, -180.0..=180.0, ui_kit::Slider { format: |v| format!("{v:.0}°"), ..Default::default() });
                            crate::scenario::publish_target("render:environment:rotation-slider", response.rect);
                            crate::scenario::publish_target("render:environment:rotation", response.rect);
                            response
                        }) {
                            request = Some(json!({"rotation_deg":value}));
                        }
                    });
                });
                ui_kit::row(ui, app.tr("render.env_intensity"), |ui| {
                    if let Some(value) = edit(app, ui, "intensity", intensity, |ui, value| {
                        let response =
                            ui_kit::slider(ui, value, 0.0..=maximum, ui_kit::Slider { format: |v| format!("{v:.2}"), ..Default::default() });
                        crate::scenario::publish_target("render:environment:intensity", response.rect);
                        response
                    }) {
                        request = Some(json!({"intensity":value}));
                    }
                });
                if let Some(request) = request
                    && let Err(error) = commit_environment(app, request)
                {
                    app.set_status(error, true);
                }
            });
            if valid && !rotates {
                note(app, ui, "render.environment.rotation_hdr");
            }
            if !valid {
                let reason = snapshot.err().unwrap_or_else(|| app.tr("render.env_choose_source").into());
                ui_kit::text(ui, &reason, ui_kit::Text::Note("render:environment:unavailable"));
            }
        });
        backdrop(app, ui);
        ground(app, ui);
        atmosphere(app, ui);
    });
    stage_confirmation(app, ui.ctx());
}

pub(super) fn open_sources(app: &mut SolveApp) {
    super::library::open_category(app, "hdr");
    if let Err(error) = app.run("view.render_stage", json!({"stage":Stage::Light.key()})) {
        app.set_status(error, true);
    }
}

fn note(app: &SolveApp, ui: &mut egui::Ui, key: &str) {
    ui_kit::text(ui, app.tr(key), ui_kit::Text::Note(key));
}

pub(super) fn persist(app: &mut SolveApp) {
    let file = hdr::active(app).and_then(|target| match hdr::input(app, target) {
        input @ hdr::Input::FileCaptured { .. } => Some((target, input.clone())),
        _ => None,
    });
    app.render_workspace.view.dirty = true;
    if let Err(error) = setups::save(app) {
        app.set_status(error, true);
    }
    // Keep the checked path available for rechecking; the engine has captured its bytes.
    if let Some((target, input)) = file {
        match target {
            hdr::Target::Gpu => app.render_workspace.view.studio.hdr = input,
            hdr::Target::Cpu => app.render_workspace.view.cpu.hdr = input,
        }
    }
}

fn source(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.env_source").to_string();
    ui_kit::section(ui, "render_environment_source", &title, true, |ui| {
        let method = app.render_workspace.view.method;
        let gpu = method == ImageMethod::GpuRaytrace;
        let target = if gpu { hdr::Target::Gpu } else { hdr::Target::Cpu };
        let selected = match method {
            ImageMethod::GpuRaytrace => match app.render_workspace.view.studio.environment {
                studio::Environment::None => 0,
                studio::Environment::Sky => 1,
                studio::Environment::Hdr => {
                    if hdr::input(app, target).embedded() {
                        3
                    } else {
                        2
                    }
                }
            },
            ImageMethod::CpuRaytrace => match app.render_workspace.view.cpu.environment {
                cpu::Environment::None => 0,
                cpu::Environment::Constant => 1,
                cpu::Environment::Hdr => 2,
            },
            ImageMethod::Raster => 0,
        };
        let keys = if gpu {
            ["render.environment.none", "render.studio_sky", "render.environment.hdr_file", "render.environment.builtin"]
        } else {
            ["render.environment.none", "render.environment.constant", "render.environment.hdr", "render.environment.builtin"]
        };
        if selected >= 2 && hdr::input(app, target).embedded() {
            let name = source_name(app, ui.ctx());
            ui_kit::banner(ui, ui_kit::AssistantTone::Neutral, |ui| {
                ui_kit::text(ui, &name, ui_kit::Text::NameCell(true));
                let kind = app.tr(if gpu { "render.environment.builtin" } else { "render.environment.hdr_file" });
                let detail = hdr::metadata(app, target).map_or_else(|| kind.to_string(), |info| format!("{kind} · {} × {}", info.width, info.height));
                ui_kit::text(ui, &detail, ui_kit::Text::Inline);
                note(app, ui, "render.environment.embedded");
            });
        }
        let mut changed = false;
        ui_kit::row(ui, app.tr("render.env_source"), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let change = ui_kit::button(ui, app.tr("render.env_change"), ui_kit::Button::Normal);
                crate::scenario::publish_target("render:environment:change", change.rect);
                if change.clicked() {
                    open_sources(app);
                }

                ui.add_enabled_ui(method != ImageMethod::Raster, |ui| {
                    ui_kit::dropdown_menu(ui, "render_environment_source_kind", app.tr(keys.get(selected).copied().unwrap_or(keys[0])), |ui| {
                        for (index, key) in keys.iter().enumerate().take(if gpu { 4 } else { 3 }) {
                            if ui_kit::list_picker(ui, key, app.tr(key), index == selected).clicked() {
                                if index == 3 {
                                    open_sources(app);
                                } else if index != selected {
                                    if gpu {
                                        app.render_workspace.view.studio.environment = match index {
                                            1 => studio::Environment::Sky,
                                            2 => studio::Environment::Hdr,
                                            _ => studio::Environment::None,
                                        };
                                        if index == 2 {
                                            app.render_workspace.view.studio.hdr = hdr::Input::Empty;
                                        }
                                    } else {
                                        app.render_workspace.view.cpu.environment = match index {
                                            1 => cpu::Environment::Constant,
                                            2 => cpu::Environment::Hdr,
                                            _ => cpu::Environment::None,
                                        };
                                        if index == 0 {
                                            app.render_workspace.view.raytrace.environment = None;
                                        }
                                    }
                                    changed = true;
                                }
                                ui.close();
                            }
                        }
                    });
                });
            });
        });
        if method == ImageMethod::Raster {
            note(app, ui, "render.environment.source_raytrace");
        }
        if selected == 1 && gpu {
            for (key, index) in [("render.environment.zenith", 0), ("render.environment.horizon", 1), ("render.environment.sky_ground", 2)] {
                let gradient = app.render_workspace.view.studio.gradient;
                let mut value = match index {
                    0 => gradient.zenith,
                    1 => gradient.horizon,
                    _ => gradient.ground,
                }
                .map(f64::from);
                if ui_kit::row(ui, app.tr(key), |ui| ui_kit::linear_color_field(ui, &mut value, 1e6)).changed() {
                    let gradient = &mut app.render_workspace.view.studio.gradient;
                    let field = match index {
                        0 => &mut gradient.zenith,
                        1 => &mut gradient.horizon,
                        _ => &mut gradient.ground,
                    };
                    *field = value.map(|v| v as f32);
                    changed = true;
                }
            }
            note(app, ui, "render.environment.sky_note");
        } else if selected == 1 {
            let mut color = app.render_workspace.view.cpu.color;
            if ui_kit::row(ui, app.tr("render.env_colour"), |ui| ui_kit::linear_color_field(ui, &mut color, 1.0)).changed() {
                app.render_workspace.view.cpu.color = color;
                changed = true;
            }
        } else if selected >= 2 {
            let before = hdr::input(app, target).clone();
            hdr::controls(app, ui, target);
            changed |= &before != hdr::input(app, target);
            let label = app.tr("render.environment.primaries").to_string();
            ui.add_enabled_ui(gpu, |ui| {
                changed |= ui_kit::checkbox(ui, &label, &mut app.render_workspace.view.studio.assume_linear_srgb).changed();
            });
            if !gpu {
                note(app, ui, "render.environment.primaries_gpu");
            }
            ui.add_enabled_ui(!gpu, |ui| {
                let samples = app.render_workspace.view.cpu.samples;
                ui_kit::row(ui, app.tr("render.environment.lighting_samples"), |ui| {
                    ui_kit::dropdown_menu(ui, "render_environment_lighting_samples", &samples.to_string(), |ui| {
                        for count in [1, 4, 16, 64] {
                            if ui_kit::list_picker(ui, &count.to_string(), &count.to_string(), count == samples).clicked() {
                                app.render_workspace.view.cpu.samples = count;
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                });
            });
            if gpu {
                note(app, ui, "render.environment.samples_cpu");
            }
        }
        if changed {
            app.render_workspace.view.dirty = true;
            // An unchecked path stays in the existing draft until the user checks it.
            if hdr::active(app).is_none_or(|target| hdr::input(app, target).resource().is_some()) {
                persist(app);
            }
        }
    });
}

fn source_name(app: &mut SolveApp, ctx: &egui::Context) -> String {
    let source = match app.render_workspace.view.method {
        ImageMethod::CpuRaytrace => {
            app.render_workspace.view.cpu.hdr.resource().filter(|_| app.render_workspace.view.cpu.environment == cpu::Environment::Hdr)
        }
        ImageMethod::GpuRaytrace => {
            app.render_workspace.view.studio.hdr.resource().filter(|_| app.render_workspace.view.studio.environment == studio::Environment::Hdr)
        }
        ImageMethod::Raster => None,
    };
    if let Some(source) = source {
        type Names = std::sync::Arc<std::collections::BTreeMap<String, String>>;
        let cache = egui::Id::new("render_environment_catalog_names");
        let names = ctx.data_mut(|data| data.get_temp::<Names>(cache)).unwrap_or_else(|| {
            let mut names = std::collections::BTreeMap::new();
            if let Ok(reply) = app.run("render.library.list", json!({"kind":"hdr","limit":128})) {
                for entry in reply["items"].as_array().into_iter().flatten().take(128) {
                    if let Some(id) = entry["id"].as_str()
                        && let Ok(detail) = app.run("render.library.describe", json!({"kind":"hdr","id":id}))
                        && let (Some(digest), Some(name)) = (detail["resource"]["sha256"].as_str(), detail["resource"]["name"].as_str())
                    {
                        names.insert(digest.to_string(), name.to_string());
                    }
                }
            }
            let names = std::sync::Arc::new(names);
            ctx.data_mut(|data| data.insert_temp(cache, names.clone()));
            names
        });
        if let Some(name) = names.get(&source.to_string()) {
            return name.clone();
        }
    }
    super::scene::environment(app).0
}

fn environment_values(app: &SolveApp) -> Option<(f64, f64, f64, bool)> {
    let v = &app.render_workspace.view;
    match v.method {
        ImageMethod::GpuRaytrace if v.studio.environment != studio::Environment::None => {
            Some((wrapped_degrees(v.studio.rotation_rad), v.studio.intensity, 100.0, true))
        }
        ImageMethod::CpuRaytrace if v.cpu.environment != cpu::Environment::None => {
            Some((wrapped_degrees(v.cpu.rotation_rad), v.cpu.intensity, 64.0, v.cpu.environment == cpu::Environment::Hdr))
        }
        _ => None,
    }
}

fn wrapped_degrees(radians: f64) -> f64 {
    let degrees = radians.to_degrees();
    if degrees.is_finite() { (degrees + 180.0).rem_euclid(360.0) - 180.0 } else { 0.0 }
}

pub(super) fn commit_environment(app: &mut SolveApp, mut request: Value) -> Result<(), String> {
    let fields = request.as_object_mut().ok_or_else(|| app.tr("render.env_invalid_edit").to_string())?;
    fields.insert("name".into(), json!(app.render_workspace.view.name));
    fields.insert("set".into(), json!(setups::snapshot(app)?));
    app.run("render.environment.set", request)?;
    setups::load(app);
    Ok(())
}

fn save(app: &mut SolveApp, setup: RenderSetup) -> Result<(), String> {
    app.run("render.settings.set", json!({"name":app.render_workspace.view.name,"set":setup}))?;
    setups::load(app);
    Ok(())
}

pub(super) fn commit_backdrop(app: &mut SolveApp, color: [u8; 3], transparent: bool) -> Result<(), String> {
    let mut setup = setups::snapshot(app)?;
    setup.background = color;
    match &mut setup.job {
        Some(RenderJob::Raster { transparent_background }) => *transparent_background = transparent,
        Some(RenderJob::GpuPath { transparent_background, options }) => {
            *transparent_background = transparent;
            options.backdrop = None;
        }
        _ if transparent => return Err(app.tr("render.cpu_opaque_required").into()),
        _ => {}
    }
    save(app, setup)
}

// Keep an in-progress gesture outside the design. Release/Enter commits once;
// external commands, undo and document switches invalidate its temporary identity.
fn edit<T: Clone + PartialEq + Send + Sync + 'static>(
    app: &SolveApp,
    ui: &mut egui::Ui,
    key: &str,
    current: T,
    draw: impl FnOnce(&mut egui::Ui, &mut T) -> egui::Response,
) -> Option<T> {
    let id =
        ui.id().with(("render-environment", key, &app.render_workspace.view.name, std::sync::Arc::as_ptr(&app.session.doc), app.session.revision));
    let (_, mut value, mut pending) = ui
        .data_mut(|data| data.get_temp::<(T, T, bool)>(id))
        .filter(|(original, _, _)| original == &current)
        .unwrap_or((current.clone(), current.clone(), false));
    let response = draw(ui, &mut value);
    pending |= response.changed();
    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    let finished =
        response.drag_stopped() || response.lost_focus() || (!ui.input(|input| input.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input());
    if escape || (pending && finished) {
        ui.data_mut(|data| data.remove::<(T, T, bool)>(id));
        return (!escape).then_some(value);
    }
    ui.data_mut(|data| data.insert_temp(id, (current, value, pending)));
    None
}

fn backdrop(app: &mut SolveApp, ui: &mut egui::Ui) {
    use solidcraft_engine::render::job::finishing::BackdropMode;
    let title = app.tr("render.env_backdrop").to_string();
    ui_kit::section(ui, "render_environment_backdrop", &title, true, |ui| {
        let gpu = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
        let mut mode = if app.render_workspace.view.transparent {
            3
        } else if app.render_workspace.view.studio.backdrop_enabled && gpu {
            2
        } else {
            1
        };
        let before_mode = mode;
        ui_kit::segmented(
            ui,
            &mut mode,
            &[app.tr("render.environment.world"), app.tr("render.env_colour"), app.tr("render.environment.gradient"), app.tr("render.env_clear")],
            ui_kit::Segments {
                enabled: &[false, true, gpu && !app.render_workspace.view.transparent, app.render_workspace.view.method != ImageMethod::CpuRaytrace],
                targets: &["render:environment:world", "render:environment:colour", "render:environment:gradient", "render:environment:clear"],
                ..Default::default()
            },
        );
        if mode == 2 && !gpu {
            app.set_status(app.tr("render.environment.needs_gpu").to_string(), true);
            mode = before_mode;
        }
        if mode != before_mode {
            let result = if mode == 2 {
                commit_gradient(app, true, app.render_workspace.view.studio.backdrop)
            } else {
                commit_backdrop(app, app.render_workspace.view.draft.background, mode == 3)
            };
            if let Err(error) = result {
                app.set_status(error, true);
            }
        }
        if mode == 1 {
            let color = edit(app, ui, "backdrop-colour", app.render_workspace.view.draft.background, |ui, value| {
                ui_kit::row(ui, app.tr("render.env_colour"), |ui| {
                    let response = ui_kit::color_field(ui, value);
                    crate::scenario::publish_target("render:environment:backdrop-colour", response.rect);
                    response
                })
            });
            if let Some(color) = color
                && let Err(error) = commit_backdrop(app, color, false)
            {
                app.set_status(error, true);
            }
        }
        if mode == 2 || !gpu {
            let mut gradient = app.render_workspace.view.studio.backdrop;
            let mut changed = false;
            ui.add_enabled_ui(gpu && !app.render_workspace.view.transparent, |ui| {
                let mut mode = match gradient.mode {
                    BackdropMode::Vertical => 0,
                    BackdropMode::Horizontal => 1,
                    BackdropMode::Radial => 2,
                };
                changed |= ui_kit::row(ui, app.tr("render.environment.mode"), |ui| {
                    ui_kit::segmented(
                        ui,
                        &mut mode,
                        &[app.tr("render.environment.vertical"), app.tr("render.environment.horizontal"), app.tr("render.environment.radial")],
                        Default::default(),
                    )
                });
                gradient.mode = match mode {
                    1 => BackdropMode::Horizontal,
                    2 => BackdropMode::Radial,
                    _ => BackdropMode::Vertical,
                };
                for (key, value) in [("render.environment.start", &mut gradient.first_srgb), ("render.environment.end", &mut gradient.second_srgb)] {
                    if let Some(color) = edit(app, ui, key, *value, |ui, value| ui_kit::row(ui, app.tr(key), |ui| ui_kit::color_field(ui, value))) {
                        *value = color;
                        changed = true;
                    }
                }
                ui.add_enabled_ui(gradient.mode == BackdropMode::Radial, |ui| {
                    for (index, key) in [(0, "render.backdrop_x"), (1, "render.backdrop_y")] {
                        if let Some(value) = gradient.center.get_mut(index)
                            && let Some(number) = edit(app, ui, key, *value, |ui, value| {
                                ui_kit::row(ui, app.tr(key), |ui| ui_kit::number_field(ui, value, 0.0..=1.0, 0.01, ""))
                            })
                        {
                            *value = number;
                            changed = true;
                        }
                    }
                    if let Some(number) = edit(app, ui, "gradient-radius", gradient.radius, |ui, value| {
                        ui_kit::row(ui, app.tr("render.backdrop_radius"), |ui| ui_kit::number_field(ui, value, 0.05..=4.0, 0.01, ""))
                    }) {
                        gradient.radius = number;
                        changed = true;
                    }
                });
                if gradient.mode != BackdropMode::Radial {
                    note(app, ui, "render.environment.radial_only");
                }
            });
            if changed && let Err(error) = commit_gradient(app, true, gradient) {
                app.set_status(error, true);
            }
        }
        ui.add_enabled_ui(false, |ui| {
            ui_kit::row(ui, app.tr("render.env_blur"), |ui| {
                ui_kit::slider(ui, &mut 0.0, 0.0..=100.0, ui_kit::Slider { format: |v| format!("{v:.0} %"), ..Default::default() })
            });
        });
        note(app, ui, "render.environment.blur_pending");
        note(app, ui, "render.environment.backdrop_note");
        note(app, ui, "render.environment.world_pending");
        if !gpu {
            note(app, ui, "render.environment.gradient_gpu");
        }
        if app.render_workspace.view.method == ImageMethod::CpuRaytrace {
            note(app, ui, "render.cpu_opaque_required");
        }
    });
}

pub(super) fn commit_gradient(
    app: &mut SolveApp,
    enabled: bool,
    gradient: solidcraft_engine::render::job::finishing::Backdrop,
) -> Result<(), String> {
    let mut setup = setups::snapshot(app)?;
    match &mut setup.job {
        Some(RenderJob::GpuPath { options, .. }) => options.backdrop = enabled.then_some(gradient),
        _ => return Err(app.tr("render.environment.needs_gpu").into()),
    }
    save(app, setup)
}

#[derive(Clone)]
pub(super) struct Ground {
    pub margin: f64,
    pub gap: f64,
    pub color: [f64; 3],
}

fn ground_values(app: &SolveApp, setup: &RenderSetup) -> Ground {
    let mut value = Ground { margin: 0.25, gap: 0.1, color: [0.18; 3] };
    if let Some(panel) = setup.stage.iter().find(|panel| matches!(panel.id.as_str(), "ground" | "studio_ground")) {
        value.color = panel.reflectance_linear;
        if let Ok(scene) = solidcraft_engine::product_scene::extract(&app.session)
            && !scene.bounds.is_empty()
        {
            let bounds = scene.bounds;
            let size = bounds.max - bounds.min;
            let extent = size.x.max(size.y).max(size.z).max(0.001);
            value.margin = ((panel.half_u_mm.first().copied().unwrap_or(0.0).abs() - (size.x * 0.5).max(0.001)) / extent).clamp(0.0, 10.0);
            value.gap = (bounds.min.z - panel.center_mm.get(2).copied().unwrap_or(bounds.min.z)).clamp(0.0001, 1000.0);
        }
    }
    value
}

pub(super) fn commit_ground(app: &mut SolveApp, enabled: bool, value: Ground) -> Result<(), String> {
    let set = setups::snapshot(app)?;
    let request = if enabled {
        json!({"name":app.render_workspace.view.name,"set":set,"margin_fraction":value.margin,"gap_mm":value.gap,"reflectance_linear":value.color,"replace_stage":true})
    } else {
        json!({"name":app.render_workspace.view.name,"set":set})
    };
    app.run(if enabled { "render.stage.ground" } else { "render.stage.clear" }, request)?;
    setups::load(app);
    Ok(())
}

#[derive(Clone)]
struct GroundForm {
    value: Ground,
    replace: bool,
}

#[derive(Clone)]
struct StageRemoval {
    document: Weak<solidcraft_engine::doc::Document>,
    revision: u64,
    name: String,
    set: RenderSetup,
}

pub(super) fn request_stage_removal(app: &SolveApp, ctx: &egui::Context) {
    if let Ok(set) = setups::snapshot(app) {
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("render_remove_stage"),
                StageRemoval {
                    document: Arc::downgrade(&app.session.doc),
                    revision: app.session.revision,
                    name: app.render_workspace.view.name.clone(),
                    set,
                },
            )
        });
    }
}

pub(super) fn stage_confirmation(app: &mut SolveApp, ctx: &egui::Context) {
    let id = egui::Id::new("render_remove_stage");
    let Some(pending) = ctx.data_mut(|data| data.get_temp::<StageRemoval>(id)) else { return };
    if pending.revision != app.session.revision
        || pending.name != app.render_workspace.view.name
        || !Weak::ptr_eq(&pending.document, &Arc::downgrade(&app.session.doc))
        || setups::snapshot(app).as_ref() != Ok(&pending.set)
    {
        ctx.data_mut(|data| data.remove::<StageRemoval>(id));
        return;
    }
    let title = app.tr("render.environment.remove_confirm").to_string();
    let remove = app.tr("render.environment.remove").to_string();
    let cancel = app.tr("render.environment.keep").to_string();
    let detail = crate::i18n::Locale::for_language(&app.preferences.language)
        .format("render.environment.remove_note", &[("count", &pending.set.stage.len().to_string())]);
    let (closed, action) =
        ui_kit::Confirm::new(id, "delete", &title).tone(ui_kit::Tone::Danger).actions(&remove, &cancel, None).show(ctx, true, |ui| {
            ui_kit::text(ui, &detail, ui_kit::Text::Note("render.environment.remove_note"));
        });
    if closed || matches!(action, ui_kit::Action::Cancel | ui_kit::Action::Ok) {
        ctx.data_mut(|data| data.remove::<StageRemoval>(id));
    }
    if !closed && action == ui_kit::Action::Ok {
        match app.run("render.stage.clear", json!({"name":pending.name,"set":pending.set})) {
            Ok(_) => setups::load(app),
            Err(error) => app.set_status(error, true),
        }
    }
}

fn ground(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.environment.ground").to_string();
    ui_kit::section(ui, "render_environment_ground", &title, true, |ui| {
        let snapshot = setups::snapshot(app);
        let id = ui.id().with(("ground-form", Arc::as_ptr(&app.session.doc), app.session.revision, &app.render_workspace.view.name));
        let mut form = ui.data_mut(|data| data.get_temp::<GroundForm>(id)).unwrap_or_else(|| GroundForm {
            value: snapshot.as_ref().map(|set| ground_values(app, set)).unwrap_or(Ground { margin: 1.0, gap: 0.1, color: [0.18; 3] }),
            replace: false,
        });
        let supported = app.render_workspace.view.method != ImageMethod::Raster;
        ui.add_enabled_ui(supported, |ui| {
            for (key, value, range, speed, unit) in [
                ("render.environment.margin", &mut form.value.margin, 0.0..=10.0, 0.05, "×"),
                ("render.environment.gap", &mut form.value.gap, 0.0001..=1000.0, 0.01, " mm"),
            ] {
                ui_kit::row(ui, app.tr(key), |ui| ui_kit::number_field(ui, value, range, speed, unit));
            }
            vector_row(ui, app.tr("render.environment.reflectance"), &mut form.value.color, 0.0..=1.0, 0.01, "");
            ui_kit::checkbox(ui, app.tr("render.environment.replace"), &mut form.replace);
            let ready = snapshot.as_ref().is_ok_and(|set| set.stage.is_empty() || form.replace);
            let fit = ui
                .add_enabled_ui(ready, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        ui_kit::button(ui, app.tr("render.environment.fit"), ui_kit::Button::Primary)
                    })
                    .inner
                })
                .inner;
            crate::scenario::publish_target("render:environment:fit-ground", fit.rect);
            if fit.clicked()
                && let Err(error) = commit_ground(app, true, form.value.clone())
            {
                app.set_status(error, true);
            }
            ui.horizontal(|ui| {
                for (kind, key) in [("floor_wall", "render.environment.floor_wall"), ("cyclorama", "render.environment.cyclorama")] {
                    if ui_kit::button(ui, app.tr(key), ui_kit::Button::Enabled(ready)).clicked()
                        && let Ok(set) = &snapshot
                    {
                        let request = json!({"name":app.render_workspace.view.name,"set":set,"kind":kind,"expected_revision":app.session.revision,
                            "replace_stage":form.replace,"reflectance_linear":form.value.color});
                        match app.run("render.stage.backdrop", request) {
                            Ok(_) => setups::load(app),
                            Err(error) => app.set_status(error, true),
                        }
                    }
                }
            });
        });
        if !supported {
            note(app, ui, "render.environment.stage_raytrace");
        }
        ui.horizontal(|ui| {
            let count = app.render_workspace.view.draft.stage.len();
            ui_kit::text(
                ui,
                &crate::i18n::Locale::for_language(&app.preferences.language)
                    .format("render.environment.saved_panels", &[("count", &count.to_string())]),
                ui_kit::Text::Inline,
            );
            let remove = ui_kit::icon_label_button(ui, app.tr("render.environment.remove"), "delete", count > 0 && snapshot.is_ok(), true);
            crate::scenario::publish_target("render:stage-clear", remove.rect);
            if remove.clicked() {
                request_stage_removal(app, ui.ctx());
            }
        });
        ui.data_mut(|data| data.insert_temp(id, form));
        if let Err(error) = snapshot {
            ui_kit::text(ui, &error, ui_kit::Text::Note("render:environment:ground-unavailable"));
        }
    });
}

/// Compact vector row; the shared row and number fields own spacing and interaction.
fn vector_row(ui: &mut egui::Ui, label: &str, value: &mut [f64; 3], range: std::ops::RangeInclusive<f64>, speed: f64, unit: &str) -> bool {
    ui_kit::row(ui, label, |ui| {
        ui.horizontal(|ui| {
            let mut changed = false;
            let width = ((ui.available_width() - Tokens::get().space_8) / 3.0).max(Tokens::get().space_16);
            for (index, component) in value.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.set_width(width);
                    changed |= ui_kit::number_field(ui, component, range.clone(), speed, unit).changed();
                });
            }
            changed
        })
        .inner
    })
}

/// One home for airborne controls, also used by the transitional Aging page.
pub(super) fn atmosphere(app: &mut SolveApp, ui: &mut egui::Ui) {
    let title = app.tr("render.environment.atmosphere").to_string();
    ui_kit::section_method(ui, "render_environment_atmosphere", &title, true, "GPU", |ui| {
        let gpu = app.render_workspace.view.method == ImageMethod::GpuRaytrace;
        let before = app.render_workspace.view.studio.volume;
        let mut volume = before.unwrap_or_default();
        let mut enabled = before.is_some();
        let mut changed = false;
        ui.add_enabled_ui(gpu, |ui| {
            let toggle = ui_kit::checkbox(ui, app.tr("render.environment.medium"), &mut enabled);
            crate::scenario::publish_target("render:volume-enable", toggle.rect);
            changed |= toggle.changed();
            ui.add_enabled_ui(enabled, |ui| {
                changed |= vector_row(ui, app.tr("render.environment.min"), &mut volume.min_mm, -1e9..=1e9, 1.0, "");
                changed |= vector_row(ui, app.tr("render.environment.max"), &mut volume.max_mm, -1e9..=1e9, 1.0, "");
                changed |= ui_kit::row(ui, app.tr("render.environment.extinction"), |ui| {
                    ui_kit::number_field(ui, &mut volume.extinction_per_mm, 0.0..=1000.0, 0.0001, " /mm")
                })
                .changed();
                changed |= vector_row(ui, app.tr("render.environment.albedo"), &mut volume.scattering_albedo, 0.0..=1.0, 0.01, "");
                changed |= ui_kit::row(ui, app.tr("render.environment.phase"), |ui| {
                    ui_kit::slider(ui, &mut volume.anisotropy, -0.95..=0.95, Default::default())
                })
                .changed();
            });
        });
        if !gpu {
            note(app, ui, "render.environment.needs_gpu");
        }
        if changed {
            app.render_workspace.view.studio.volume = enabled.then_some(volume);
            app.render_workspace.view.dirty = true;
            if let Err(error) = setups::save(app) {
                app.render_workspace.view.studio.volume = before;
                app.set_status(error, true);
            }
        }
    });
}
