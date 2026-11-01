//! Render scene content in the shared Browser host and row painter.
use super::{ImageMethod, RenderSelection, cpu, hdr, inspector, setups, studio};
use crate::{
    SolveApp,
    browser::{self, Row},
    context_menu::{self, Item},
    theme::Tokens,
    ui_kit,
};
use egui::Color32;
use serde_json::json;
use solidcraft_engine::{
    Sel,
    doc::{ModelBody, appearance::Look},
};

#[derive(Default)]
pub(crate) struct Decoration {
    pub swatch: Option<Color32>,
    pub name: String,
    pub count: String,
}

pub(crate) fn decoration(app: &SolveApp, names: &[String]) -> Decoration {
    if !super::active(app) {
        return Decoration::default();
    }
    let state = crate::open_meshing::session_world(&app.session);
    let bodies: Vec<_> = names.iter().filter_map(|name| state.body(name)).collect();
    let Some(body) = bodies.first() else { return Decoration::default() };
    let look = object_look(app, body);
    let mixed = bodies.iter().any(|b| object_look(app, b) != look);
    let count = bodies.iter().map(|b| app.session.doc.face_colors(b).len()).fold(0usize, usize::saturating_add);
    Decoration {
        swatch: Some(if mixed { Tokens::get().text_dim } else { look_color(app, body, look.as_ref()) }),
        name: if mixed { app.tr("render.tree_mixed").into() } else { look_name(app, look.as_ref()) },
        count: if count == 0 {
            String::new()
        } else {
            crate::i18n::format(if count == 1 { "render.tree_face_one" } else { "render.tree_face_count" }, &[("count", &count.to_string())])
        },
    }
}

fn object_look(app: &SolveApp, body: &ModelBody) -> Option<Look> {
    let face = app.session.selection.iter().rev().find_map(|s| match s {
        Sel::Face { body: name, index, .. } if name == &body.name => Some(*index),
        _ => None,
    });
    face.map_or_else(|| app.session.doc.model_display_look(body), |face| app.session.doc.model_face_look(body, face))
}

pub(crate) fn same_look(app: &SolveApp, names: &[String]) -> bool {
    let state = crate::open_meshing::session_world(&app.session);
    let mut looks = names.iter().filter_map(|name| state.body(name)).map(|body| app.session.doc.model_display_look(body));
    let Some(first) = looks.next() else { return false };
    looks.all(|look| look == first)
}

fn look_name(app: &SolveApp, look: Option<&Look>) -> String {
    look.map_or_else(|| app.tr("render.tree_default").into(), |look| look.name.clone())
}
fn look_color(app: &SolveApp, body: &ModelBody, look: Option<&Look>) -> Color32 {
    let color = look.map(|l| l.color).or_else(|| app.session.doc.body_color(body).map(|c| c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)));
    color.map_or(Tokens::get().body, |rgb| crate::theme::srgb(rgb.map(|channel| f32::from(channel) / 255.0)))
}

fn section(app: &mut SolveApp, ui: &mut egui::Ui, key: &str) -> bool {
    let id = format!("render:tree:{key}");
    let open = browser::is_open(app, &id, true);
    let label = app.tr(&format!("render.tree_{key}")).to_string();
    let add_label = app.tr(if key == "lights" { "render.tree_add_light" } else { "render.tree_add_shot" }).to_string();
    let pending = app.tr("render.light_backend_limit").to_string();
    let actions = [ui_kit::TreeAction {
        icon: "plus",
        hover_only: false,
        label: &add_label,
        reason: (key == "lights" && app.render_workspace.view.method == ImageMethod::Raster).then_some(pending.as_str()),
        target: if key == "lights" { "render:tree:lights:add" } else { "render:tree:shots:add" },
    }];
    let row = browser::draw_row(
        ui,
        ui.id().with(&id),
        &Row {
            label: &label,
            name: true,
            icon: match key {
                "lights" => "render_scene",
                "shots" => "camera",
                _ => "box",
            },
            secondary: &match key {
                "lights" => light_types(app).len(),
                "shots" => app.session.doc.render.setups.len(),
                _ => crate::open_meshing::session_world(&app.session).bodies.len(),
            }
            .to_string(),
            fold: Some(open),
            selected: key == "lights" && app.render_workspace.view.selection == RenderSelection::Lighting,
            actions: if matches!(key, "lights" | "shots") { &actions } else { &[] },
            ..Default::default()
        },
    );
    if let Some(rect) = row.rect {
        crate::scenario::publish_target(&id, rect);
    }
    if row.actions.iter().any(|r| r.clicked()) {
        // The existing Shots manager owns naming and saving. This navigation never saves.
        if key == "lights" {
            if let Err(error) = super::light::edit(app, "render.lights.add", None, Some(json!({"kind":"area"}))) {
                app.set_status(error, true);
            }
        } else {
            app.start("render.settings.get");
        }
    }
    if row.clicked && key == "lights" {
        select(app, "lighting", None);
    }
    if row.fold || (row.clicked && key != "lights") {
        browser::toggle(app, &id, true);
    }
    open
}

fn select(app: &mut SolveApp, kind: &str, id: Option<&str>) {
    let params = id.map_or_else(|| json!({"kind":kind}), |id| json!({"kind":kind,"id":id}));
    let _ = app.run("view.render_select", params);
}

pub(crate) fn light_types(app: &SolveApp) -> Vec<&'static str> {
    super::light::rows(app)
        .unwrap_or_default()
        .iter()
        .map(|light| match light.geometry {
            solidcraft_engine::schema::render_lights::Geometry::Directional { angular_radius_rad: Some(_), .. } => "render.tree_sun",
            solidcraft_engine::schema::render_lights::Geometry::Directional { .. } => "render.tree_directional",
            solidcraft_engine::schema::render_lights::Geometry::Point { .. } => "render.tree_point",
            solidcraft_engine::schema::render_lights::Geometry::Spot { .. } => "render.tree_spot",
            solidcraft_engine::schema::render_lights::Geometry::Area { .. } => "render.tree_rectangle",
        })
        .collect()
}
pub(crate) fn environment(app: &SolveApp) -> (String, String) {
    let view = &app.render_workspace.view;
    let fixed = || (app.tr("render.tree_fixed_studio").to_string(), view.draft.environment_rotation, view.draft.environment_intensity);
    let source = |input: &hdr::Input| match input {
        hdr::Input::EmbeddedResource { resource } => format!("{} {resource}", app.tr("render.tree_hdr")),
        hdr::Input::FileCaptured { path, .. } | hdr::Input::FilePending { path } => {
            std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| app.tr("render.tree_hdr").into())
        }
        hdr::Input::Empty => app.tr("render.tree_no_environment").into(),
    };
    let (name, rotation, intensity) = match view.method {
        ImageMethod::CpuRaytrace => match view.cpu.environment {
            cpu::Environment::Hdr => (source(&view.cpu.hdr), view.cpu.rotation_rad, view.cpu.intensity),
            cpu::Environment::Constant => (app.tr("render.tree_constant").into(), 0.0, view.cpu.intensity),
            cpu::Environment::None => fixed(),
        },
        ImageMethod::GpuRaytrace => match view.studio.environment {
            studio::Environment::Hdr => (source(&view.studio.hdr), view.studio.rotation_rad, view.studio.intensity),
            studio::Environment::Sky => (app.tr("render.tree_sky").into(), view.studio.rotation_rad, view.studio.intensity),
            studio::Environment::None => fixed(),
        },
        ImageMethod::Raster => fixed(),
    };
    (
        name,
        crate::i18n::format(
            "render.tree_environment_values",
            &[("rotation", &format!("{:.0}", rotation.to_degrees())), ("intensity", &format!("{intensity:.2}"))],
        ),
    )
}

pub(crate) fn sections(app: &mut SolveApp, ui: &mut egui::Ui) {
    setups::refresh_dirty(app);
    let (source, _detail) = environment(app);
    let open = browser::is_open(app, "render:tree:environment", true);
    let environment_label = app.tr("render.tree_environment").to_string();
    let row = browser::draw_row(
        ui,
        ui.id().with("render_environment"),
        &Row {
            label: &environment_label,
            name: true,
            icon: "environment_map",
            subtitle: &source,
            fold: Some(open),
            selected: app.render_workspace.view.selection == RenderSelection::Environment,
            ..Default::default()
        },
    );
    if let Some(rect) = row.rect {
        crate::scenario::publish_target("render:tree:environment", rect);
        crate::scenario::publish_target("render:tree:environment:current", rect);
    }
    if row.fold {
        browser::toggle(app, "render:tree:environment", true);
    }
    if row.clicked {
        select(app, "environment", None);
        app.render_workspace.view.inspector.page = inspector::Page::Environment;
    }
    if section(app, ui, "lights") {
        for (index, light) in super::light::rows(app).unwrap_or_default().iter().enumerate() {
            let label = super::light::name(app, light);
            let id = &light.id;
            let visible = app.render_workspace.view.lights.visibility.visible(id);
            let solo = app.render_workspace.view.lights.visibility.solo.as_deref() == Some(id);
            let solo_target = format!("render:tree:light:{index}:solo");
            let solo_label = app.tr("render.tree_solo").to_string();
            let actions = [ui_kit::TreeAction { icon: "lookat", hover_only: true, label: &solo_label, reason: None, target: &solo_target }];
            let row = browser::draw_row(
                ui,
                ui.id().with(("render_light", id)),
                &Row {
                    depth: 1,
                    label: &label,
                    name: true,
                    icon: "render_scene",
                    eye: Some(visible),
                    dim: !visible,
                    actions: &actions,
                    selected: app.render_workspace.view.selection == RenderSelection::Light(id.clone()),
                    ..Default::default()
                },
            );
            if let Some(rect) = row.rect {
                crate::scenario::publish_target(&format!("render:tree:light:{index}"), rect);
            }
            if let Some(rect) = row.eye_rect.filter(|rect| rect.is_positive()) {
                crate::scenario::publish_target(&format!("render:tree:light:{index}:mute"), rect);
            }
            if row.eye {
                if !visible && app.render_workspace.view.lights.visibility.solo.is_some() {
                    let _ = app.run("render.lights.solo", json!({"solo":false}));
                }
                let _ = app.run("render.lights.mute", json!({"id":id,"mute":visible}));
            }
            if row.actions.iter().any(|response| response.clicked()) {
                let _ = app.run("render.lights.solo", json!({"id":id,"solo":!solo}));
            }
            if row.clicked {
                select(app, "light", Some(id));
                app.render_workspace.view.inspector.page = inspector::Page::Lighting;
            }
        }
    }
    if section(app, ui, "shots") {
        let names: Vec<_> = app.session.doc.render.setups.keys().cloned().collect();
        for name in names {
            let active = name == app.render_workspace.view.name;
            let row = browser::draw_row(
                ui,
                ui.id().with(("render_shot", &name)),
                &Row {
                    depth: 1,
                    label: &name,
                    name: true,
                    icon: "camera",
                    selected: app.render_workspace.view.selection == RenderSelection::Shot(name.clone()),
                    radio: Some(active),
                    radio_hint: app.tr("render.tree_switch"),
                    modified: active && app.render_workspace.view.dirty,
                    ..Default::default()
                },
            );
            if let Some(rect) = row.rect {
                crate::scenario::publish_target(&format!("render:tree:shot:{name}"), rect);
            }
            if row.clicked || row.radio {
                select(app, "shot", Some(&name));
                super::switch_shot(app, name.clone());
            }
            if let Some(response) = row.resp {
                ui_kit::Menu::context(&response, |ui| {
                    for (command, key) in [
                        ("render.settings.apply", "render.tree_switch"),
                        ("render.shot.rename", "render.shot_rename"),
                        ("render.shot.duplicate", "render.shot_duplicate"),
                        ("render.shot.remove", "render.shot_remove"),
                    ] {
                        let item = Item::action(command, app.tr(key), "");
                        if context_menu::list_row(ui, &item).is_some() {
                            if command == "render.settings.apply" {
                                super::switch_shot(app, name.clone());
                            } else {
                                inspector::shot_row_action(app, name.clone(), command);
                            }
                            ui.close();
                        }
                    }
                });
            }
        }
    }
}

pub(crate) fn output(app: &mut SolveApp, ui: &mut egui::Ui) {
    let (size, format) = super::output_summary(app);
    let subtitle = format!("{size} · {format}");
    let row = browser::draw_row(
        ui,
        ui.id().with("render_output"),
        &Row {
            label: app.tr("render.tree_output"),
            icon: "render_image",
            name: true,
            subtitle: &subtitle,
            selected: app.render_workspace.view.selection == RenderSelection::Output,
            ..Default::default()
        },
    );
    if let Some(rect) = row.rect {
        crate::scenario::publish_target("render:tree:output", rect);
    }
    if row.clicked {
        select(app, "output", None);
    }
    for (kind, key, selection) in [
        ("sampling", "render.children.sampling", RenderSelection::Sampling),
        ("effects", "render.output_effects", RenderSelection::Effects),
        ("check", "render.children.check", RenderSelection::Check),
    ] {
        let subtitle = crate::render_workspace::output_child_summary(app, kind);
        let row = browser::draw_row(
            ui,
            ui.id().with(("render_output_child", kind)),
            &Row {
                depth: 1,
                label: app.tr(key),
                name: true,
                subtitle: &subtitle,
                selected: app.render_workspace.view.selection == selection,
                ..Default::default()
            },
        );
        if let Some(rect) = row.rect {
            crate::scenario::publish_target(&format!("render:tree:output:{kind}"), rect);
        }
        if row.clicked {
            select(app, kind, None);
        }
    }
}

#[derive(Debug)]
pub(crate) struct LookGroup {
    pub look: Option<Look>,
    pub objects: Vec<String>,
}

pub(crate) fn groups(app: &SolveApp) -> Vec<LookGroup> {
    let mut groups: Vec<LookGroup> = Vec::new();
    for body in &crate::open_meshing::session_world(&app.session).bodies {
        for look in app.session.doc.model_used_looks(body) {
            if let Some(group) = groups.iter_mut().find(|g| g.look == look) {
                group.objects.push(body.name.clone());
            } else {
                groups.push(LookGroup { look, objects: vec![body.name.clone()] });
            }
        }
    }
    groups.sort_by_key(|g| look_name(app, g.look.as_ref()));
    groups
}

pub(crate) fn wear_subtitle(app: &SolveApp) -> String {
    let count = app.render_workspace.view.studio.aging.len();
    if count == 0 {
        app.tr("render.wear_no_layers").into()
    } else {
        crate::i18n::format(if count == 1 { "render.wear_layer_one" } else { "render.wear_layer_count" }, &[("count", &count.to_string())])
    }
}

fn wear(app: &mut SolveApp, ui: &mut egui::Ui) {
    let subtitle = wear_subtitle(app);
    let row = browser::draw_row(
        ui,
        ui.id().with("render_wear"),
        &Row {
            depth: 1,
            label: app.tr("render.wear_title"),
            name: true,
            icon: "appearance",
            subtitle: &subtitle,
            selected: app.render_workspace.view.selection == RenderSelection::Wear,
            ..Default::default()
        },
    );
    if let Some(rect) = row.rect {
        crate::scenario::publish_target("render:tree:wear", rect);
    }
    if row.clicked {
        select(app, "wear", None);
    }
}

/// Returns true when the object section is closed or the By look lens drew its objects.
pub(crate) fn objects(app: &mut SolveApp, ui: &mut egui::Ui, hover_bodies: &mut Vec<String>, hovered: &mut bool) -> bool {
    if app.tree.reveal.is_some() {
        app.tree.collapsed.remove("render:tree:objects");
    }
    if !section(app, ui, "objects") {
        return true;
    }
    wear(app, ui);
    if !app.render_workspace.view.by_look {
        return false;
    }
    for (index, group) in groups(app).into_iter().enumerate() {
        let key = format!("render:look:{:?}", group.look);
        if app.tree.reveal.as_ref().is_some_and(|b| group.objects.contains(b)) {
            app.tree.collapsed.remove(&key);
        }
        let open = browser::is_open(app, &key, true);
        let label = look_name(app, group.look.as_ref());
        let count = crate::i18n::format(
            if group.objects.len() == 1 { "render.tree_user_one" } else { "render.tree_users" },
            &[("count", &group.objects.len().to_string())],
        );
        let row = browser::draw_row(
            ui,
            ui.id().with(("render_look", &key)),
            &Row {
                depth: 1,
                label: &label,
                name: true,
                fold: Some(open),
                count: &count,
                swatch: Some(
                    group.look.as_ref().map_or(Tokens::get().body, |l| crate::theme::srgb(l.color.map(|channel| f32::from(channel) / 255.0))),
                ),
                ..Default::default()
            },
        );
        if let Some(rect) = row.rect {
            crate::scenario::publish_target(&format!("render:tree:look:{index}"), rect);
        }
        if row.fold {
            browser::toggle(app, &key, true);
        }
        let source = group.objects.first().and_then(|name| {
            let state = crate::open_meshing::session_world(&app.session);
            let body = state.body(name)?;
            let face = if app.session.doc.model_display_look(body) == group.look {
                None
            } else {
                app.session.doc.face_colors(body).into_iter().find(|(_, look)| Some(look) == group.look.as_ref()).map(|(index, _)| index)
            };
            Some(json!({"body":name,"face":face}))
        });
        if row.clicked
            && let Some(params) = &source
        {
            let _ = app.run("select.same_look", params.clone());
        }
        if let Some(response) = row.resp {
            ui_kit::Menu::context(&response, |ui| {
                let item = context_menu::cmd(app, "select.same_look", "Select Same Look");
                if context_menu::list_row(ui, &item).is_some()
                    && let Some(params) = &source
                {
                    let _ = app.run("select.same_look", params.clone());
                    ui.close();
                }
            });
        }
        if open {
            for name in group.objects {
                let decoration = decoration(app, std::slice::from_ref(&name));
                if !matches_query(app, &name, &decoration.name) {
                    continue;
                }
                let visible = !app.session.visibility.hidden_bodies.contains(&name);
                let selected = app.session.selection.iter().any(|s| match s {
                    Sel::Occurrence { path } => app.session.occurrence_bodies(path, true).is_ok_and(|bodies| bodies.contains(&name)),
                    _ => browser::picked_body(s) == Some(&name),
                });
                let row = browser::draw_row(
                    ui,
                    ui.id().with((&key, &name)),
                    &Row {
                        depth: 2,
                        label: &name,
                        name: true,
                        eye: Some(visible),
                        selected,
                        dim: !app.session.body_visible(&name),
                        swatch: decoration.swatch,
                        secondary: &decoration.name,
                        count: &decoration.count,
                        ..Default::default()
                    },
                );
                if row.clicked {
                    let _ = app.run(
                        "select.set",
                        json!({"items":[Sel::Body {name:name.clone()}],"add":ui.input(|i|i.modifiers.command || i.modifiers.ctrl)}),
                    );
                }
                if row.eye {
                    let _ = app.run("browser.visibility", json!({"items":[name],"visible":!visible}));
                }
                if row.hovered {
                    *hover_bodies = vec![name.clone()];
                    *hovered = true;
                }
                if app.tree.reveal.as_ref() == Some(&name)
                    && let Some(rect) = row.rect
                {
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                }
                if let Some(at) = row.secondary {
                    context_menu::open_for(app, at, context_menu::Target::Body { name });
                }
            }
        }
    }
    true
}

pub(crate) fn matches_query(app: &SolveApp, name: &str, look: &str) -> bool {
    let query = app.render_workspace.view.object_query.trim().to_lowercase();
    !app.render_workspace.view.search_objects || query.is_empty() || name.to_lowercase().contains(&query) || look.to_lowercase().contains(&query)
}
