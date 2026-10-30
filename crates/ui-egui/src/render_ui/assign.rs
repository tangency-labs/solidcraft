//! Render-only gesture state. Preview changes the captured live scene, never the document.
use crate::{SolveApp, ui_kit, viewport::Hit};
use egui::Modifiers;
use serde_json::{Value, json};
use solidcraft_engine::{
    Sel,
    doc::appearance::Look,
    render::{FaceAppearance, ProductScene},
};
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    pub look: Option<Value>,
    pub painting: bool,
    pub sampling: bool,
    drag: Option<(Value, u64, usize)>,
    preview: Option<Preview>,
    hover: Option<Sel>,
    recent: Vec<Value>,
    context: Option<Sel>,
    replacement: Option<Replacement>,
    replacement_index: usize,
}

#[derive(Clone, Debug)]
struct Replacement {
    from: Value,
    document: std::sync::Weak<solidcraft_engine::doc::Document>,
    revision: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Preview {
    look: Value,
    targets: Value,
    faces: Vec<(String, usize)>,
    bodies: Vec<String>,
}

fn component(app: &SolveApp, sel: &Sel) -> Option<u64> {
    match sel {
        Sel::Occurrence { path } => app.session.placed_document().path_placement(path).map(|(id, _)| id).filter(|id| *id != 0),
        Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => {
            let state = app.session.world_state();
            state.body(name).map(|body| app.session.doc.model_body_component(body)).filter(|id| *id != 0)
        }
        _ => None,
    }
}

/// Build all targets first; the engine applies them atomically with one history entry.
pub(crate) fn targets(app: &SolveApp, selected: &[Sel]) -> Result<Value, String> {
    let mut bodies = std::collections::BTreeSet::new();
    let mut components = std::collections::BTreeSet::new();
    let mut faces = Vec::new();
    for sel in selected {
        if crate::selection_mode::render_filter(app.ui.pick_mode) == "component"
            && let Some(id) = component(app, sel)
        {
            components.insert(id.to_string());
            continue;
        }
        match sel {
            Sel::Face { body, point, .. } if crate::selection_mode::render_filter(app.ui.pick_mode) == "face" => {
                faces.push(json!({"body":body,"point":point}));
            }
            Sel::Body { name } => {
                bodies.insert(name.clone());
            }
            Sel::Face { body, .. } | Sel::Edge { body, .. } => {
                bodies.insert(body.clone());
            }
            Sel::Occurrence { path } => {
                bodies.extend(app.session.occurrence_bodies(path, true).map_err(|e| e.to_string())?);
            }
            _ => return Err("selection has no appearance target".into()),
        }
    }
    if bodies.is_empty() && components.is_empty() && faces.is_empty() {
        return Err("pick an object first".into());
    }
    Ok(json!({"bodies":bodies,"components":components,"faces":faces}))
}

fn chain_target(app: &SolveApp, sel: &Sel) -> Result<Value, String> {
    match sel {
        Sel::Face { body, index, .. } => Ok(json!({"body":body,"face":index})),
        Sel::Body { name } | Sel::Edge { body: name, .. } => Ok(json!({"body":name})),
        Sel::Occurrence { .. } => component(app, sel).map(|id| json!({"component":id})).ok_or_else(|| "no component at target".into()),
        _ => Err("pick an object first".into()),
    }
}

fn remember(app: &mut SolveApp, look: Value) {
    let recent = &mut app.render_workspace.view.assign.recent;
    recent.retain(|l| l != &look);
    recent.insert(0, look);
    recent.truncate(6);
}

fn apply(app: &mut SolveApp, look: &Value, targets: Value) -> Result<(), String> {
    let mut params = look.clone();
    let object = params.as_object_mut().ok_or("invalid look")?;
    for (key, value) in targets.as_object().into_iter().flatten() {
        object.insert(key.clone(), value.clone());
    }
    app.run("appearance.assign", params)?;
    Ok(())
}

pub(crate) fn arm(app: &mut SolveApp, look: Value) -> Result<(), String> {
    let look = app.run("appearance.paint", look)?["look"].clone();
    app.tool = None;
    app.render_workspace.view.result = false;
    app.render_workspace.view.assign.look = Some(look);
    app.render_workspace.view.assign.painting = true;
    app.render_workspace.view.assign.sampling = false;
    Ok(())
}

pub(crate) fn tile(app: &mut SolveApp, look: Value, drag: bool) -> Result<(), String> {
    let look = app.run("appearance.paint", look)?["look"].clone();
    app.render_workspace.view.assign.look = Some(look.clone());
    if drag {
        let identity = std::sync::Arc::as_ptr(&app.session.doc) as usize;
        app.render_workspace.view.assign.drag = Some((look, app.session.revision, identity));
        // A Ctrl-drop paints a part; the drag ends if the design changes, so one walk serves it.
        crate::browser::prepare_body_paths(app);
        app.render_workspace.view.assign.painting = false;
        app.render_workspace.view.assign.sampling = false;
        Ok(())
    } else if app.session.selection.is_empty() {
        arm(app, look)
    } else {
        apply(app, &look, targets(app, &app.session.selection)?)
    }
}

pub(crate) fn stop(app: &mut SolveApp) {
    let state = &mut app.render_workspace.view.assign;
    state.painting = false;
    state.sampling = false;
    state.drag = None;
    state.preview = None;
    state.hover = None;
    state.replacement = None;
}

/// The tool owns the first Escape before the shell's ordinary selection clearing.
pub(crate) fn cancel(app: &mut SolveApp) -> bool {
    let state = &app.render_workspace.view.assign;
    if !super::active(app) || !(state.painting || state.sampling || state.drag.is_some()) {
        return false;
    }
    let _ = app.run("appearance.paint", json!({"done":true}));
    stop(app);
    true
}

pub(crate) fn start(app: &mut SolveApp, id: &str) -> bool {
    if id == "appearance.library" && super::active(app) {
        super::library::focus_library(app);
        return true;
    }
    if !matches!(
        id,
        "appearance.paint"
            | "appearance.pick_up"
            | "appearance.copy"
            | "appearance.paste"
            | "appearance.chain"
            | "appearance.replace"
            | "appearance.favorite"
    ) {
        return false;
    }
    let result = (|| {
        if !super::active(app) {
            super::enter(app, true)?;
        }
        if id == "appearance.paint" {
            let look = app.render_workspace.view.assign.look.clone().ok_or_else(|| app.tr("render.choose_look").to_string())?;
            arm(app, look)
        } else if id == "appearance.pick_up" {
            app.run(id, json!({}))?;
            stop(app);
            app.tool = None;
            app.render_workspace.view.assign.sampling = true;
            Ok(())
        } else if id == "appearance.copy" {
            let sel = app.session.selection.last().ok_or("pick an object first")?;
            let target = chain_target(app, sel)?;
            app.run(id, json!({"target":target}))?;
            Ok(())
        } else if id == "appearance.paste" {
            let target = targets(app, &app.session.selection)?;
            app.run(id, target)?;
            Ok(())
        } else {
            let sel = app.session.selection.last().ok_or("pick an object first")?;
            let target = chain_target(app, sel)?;
            if id == "appearance.chain" {
                app.run(id, json!({"target":target}))?;
                app.run("view.render_stage", json!({"stage":"look"}))?;
            } else {
                let look = app.run("appearance.pick_up", json!({"target":target}))?["look"].clone();
                if id == "appearance.replace" {
                    app.render_workspace.view.assign.replacement =
                        Some(Replacement { from: look, document: std::sync::Arc::downgrade(&app.session.doc), revision: app.session.revision });
                    app.render_workspace.view.assign.replacement_index = 0;
                } else {
                    let favorite = !is_favorite(app, &look);
                    app.run(id, json!({"look":look, "favorite":favorite}))?;
                }
            }
            Ok(())
        }
    })();
    if let Err(error) = result {
        app.set_status(error, true);
    }
    true
}

pub(crate) fn candidate(app: &SolveApp, hits: &[Hit], mods: Modifiers) -> Option<(Hit, Sel)> {
    if app.render_workspace.view.assign.drag.is_some() {
        let hit = hits.iter().find(|h| matches!(h, Hit::Face { .. }))?;
        if let Hit::Face { body, index, point } = hit {
            let sel = if mods.shift {
                Sel::Face { body: body.clone(), index: *index, point: *point }
            } else if mods.ctrl || mods.command {
                crate::selection_mode::part_of(app, body)
            } else {
                return crate::viewport::candidate(app, std::slice::from_ref(hit));
            };
            return Some((hit.clone(), sel));
        }
    }
    crate::viewport::candidate(app, hits)
}

fn drop_preview(app: &SolveApp, look: Value, hit: &Hit, sel: &Sel, mods: Modifiers) -> Result<Preview, String> {
    let Hit::Face { body, index, point } = hit else {
        return Err("no face under cursor".into());
    };
    let face = mods.shift || (!mods.ctrl && !mods.command && crate::selection_mode::render_filter(app.ui.pick_mode) == "face");
    if face {
        return Ok(Preview {
            look,
            targets: json!({"faces":[{"body":body,"point":point}]}),
            faces: vec![(body.clone(), *index)],
            bodies: Vec::new(),
        });
    }
    let whole_component = mods.ctrl || mods.command || crate::selection_mode::render_filter(app.ui.pick_mode) == "component";
    if whole_component && let Some(id) = component(app, sel) {
        let bodies = app
            .session
            .world_state()
            .bodies
            .iter()
            .filter(|b| {
                app.session.doc.component_within(app.session.doc.model_body_component(b), id)
                    && !app.session.doc.appearances.bodies.contains_key(b.definition_name())
            })
            .map(|b| b.name.clone())
            .collect();
        return Ok(Preview { look, targets: json!({"components":[id.to_string()]}), faces: Vec::new(), bodies });
    }
    Ok(Preview { look, targets: json!({"bodies":[body]}), faces: Vec::new(), bodies: vec![body.clone()] })
}

fn click(app: &mut SolveApp, candidate: Option<&(Hit, Sel)>) -> Result<(), String> {
    let Some((hit, sel)) = candidate else {
        return Ok(());
    };
    if app.render_workspace.view.assign.sampling {
        // Sample the actual visible face even when the Pick size is Body.
        let source = match hit {
            Hit::Face { body, index, .. } => json!({"body":body,"face":index}),
            _ => chain_target(app, sel)?,
        };
        let reply = app.run("appearance.pick_up", json!({"target":source}))?;
        return arm(app, reply["look"].clone());
    }
    if app.render_workspace.view.assign.painting
        && let Some(look) = app.render_workspace.view.assign.look.clone()
    {
        let target = drop_preview(app, look.clone(), hit, sel, Modifiers::NONE)?;
        apply(app, &look, target.targets)?;
    }
    Ok(())
}

pub(crate) fn input(app: &mut SolveApp, ui: &mut egui::Ui, response: &egui::Response, candidate: Option<&(Hit, Sel)>, inside: bool) -> bool {
    let active = {
        let s = &app.render_workspace.view.assign;
        s.painting || s.sampling || s.drag.is_some()
    };
    if !active {
        return false;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        let _ = app.run("appearance.paint", json!({"done":true}));
        stop(app);
        app.esc_handled = true;
        return true;
    }
    if let Some((look, revision, identity)) = app.render_workspace.view.assign.drag.clone() {
        if revision != app.session.revision
            || identity != std::sync::Arc::as_ptr(&app.session.doc) as usize
            || ui.input(|i| !i.pointer.primary_down() && !i.pointer.button_released(egui::PointerButton::Primary))
        {
            stop(app);
            return true;
        }
        let mods = ui.input(|i| i.modifiers);
        app.render_workspace.view.assign.hover = candidate.filter(|_| inside).map(|(_, sel)| sel.clone());
        app.render_workspace.view.assign.preview = candidate.filter(|_| inside).and_then(|(hit, sel)| drop_preview(app, look, hit, sel, mods).ok());
        if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
            let preview = app.render_workspace.view.assign.preview.take();
            app.render_workspace.view.assign.drag = None;
            app.render_workspace.view.assign.hover = None;
            if let Some(preview) = preview
                && let Err(error) = apply(app, &preview.look, preview.targets)
            {
                app.set_status(error, true);
            }
        }
    } else if inside
        && response.clicked_by(egui::PointerButton::Primary)
        && let Err(error) = click(app, candidate)
    {
        app.set_status(error, true);
    }
    let state = &app.render_workspace.view.assign;
    if (state.painting || state.drag.is_some())
        && let Some(look) = &state.look
    {
        ui_kit::sphere_cursor(ui, super::library::tile_sphere(look));
    }
    true
}

pub(crate) fn banner(app: &mut SolveApp, ui: &mut egui::Ui) {
    let s = &app.render_workspace.view.assign;
    if !s.painting && !s.sampling {
        return;
    }
    let text = if s.sampling {
        app.tr("render.pick_up_prompt").to_string()
    } else {
        crate::i18n::format("render.painting_with", &[("name", s.look.as_ref().and_then(|l| l["name"].as_str()).unwrap_or(""))])
    };
    ui_kit::text(ui, &text, ui_kit::Text::Inline);
    if ui_kit::button(ui, app.tr_label("Done"), ui_kit::Button::Normal).clicked() {
        let _ = app.run("appearance.paint", json!({"done":true}));
        stop(app);
    }
}

pub(crate) fn preview(app: &SolveApp) -> Option<Preview> {
    app.render_workspace.view.assign.preview.clone()
}
pub(crate) fn preview_key(app: &SolveApp) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    if let Some(p) = &app.render_workspace.view.assign.preview {
        p.look.to_string().hash(&mut hash);
        p.targets.to_string().hash(&mut hash);
    }
    hash.finish()
}

pub(crate) fn preview_scene(scene: &mut ProductScene, preview: Option<&Preview>) -> Result<(), String> {
    let Some(preview) = preview else {
        return Ok(());
    };
    let color = crate::dialogs_appearance::parse_color(&preview.look["color"]).ok_or("invalid preview look")?;
    let look = Look {
        name: preview.look["name"].as_str().unwrap_or("").into(),
        color,
        opacity: preview.look["opacity"].as_f64().unwrap_or(1.0),
        pbr: preview.look.get("pbr").filter(|p| !p.is_null()).map(|p| serde_json::from_value(p.clone())).transpose().map_err(|e| e.to_string())?,
    };
    let surface = look.opaque_surface().map_err(|e| e.to_string())?;
    for mesh in &mut scene.meshes {
        if preview.bodies.contains(&mesh.name) {
            mesh.color = look.color;
            mesh.opacity = look.opacity;
            mesh.surface = surface;
        }
        for (_, face) in preview.faces.iter().filter(|(name, _)| name == &mesh.name) {
            let face = u32::try_from(*face).map_err(|_| "invalid preview face")?;
            mesh.faces.retain(|f| f.face != face);
            mesh.faces.push(FaceAppearance { face, color: look.color, opacity: look.opacity, surface, transmission: None });
        }
    }
    Ok(())
}

pub(crate) fn menu(app: &SolveApp, body: &str) -> crate::context_menu::Item {
    use crate::context_menu::{Item, cmd};
    let selected_face = app
        .render_workspace
        .view
        .assign
        .context
        .as_ref()
        .filter(|s| matches!(s, Sel::Face {body:name,..} if name == body))
        .or_else(|| app.session.selection.iter().find(|s| matches!(s,Sel::Face {body:name,..} if name == body)));
    let sel = selected_face.cloned().unwrap_or(Sel::Body { name: body.into() });
    let targets = targets(app, std::slice::from_ref(&sel)).unwrap_or_else(|_| json!({"bodies":[body]}));
    let mut menu = Item::action("appearance.assign", app.tr("cmd.appearance_assign"), "");
    menu.children.push(Item::heading(app.tr("render.recent_looks")));
    for look in &app.render_workspace.view.assign.recent {
        let mut params = look.clone();
        if let Some(object) = params.as_object_mut() {
            for (k, v) in targets.as_object().into_iter().flatten() {
                object.insert(k.clone(), v.clone());
            }
        }
        menu.children.push(cmd(app, "appearance.assign", look["name"].as_str().unwrap_or("")).with(params));
    }
    menu.children.push(Item::sep());
    menu.children.push(cmd(app, "appearance.library", app.tr("render.browse_looks")));
    menu.children.push(cmd(app, "appearance.assign", app.tr("cmd.appearance_assign")));
    menu.children.push(cmd(app, "appearance.chain", app.tr("cmd.appearance_chain")));
    menu.children.push(cmd(app, "appearance.replace", app.tr("render.replace_all")));
    menu.children.push(cmd(app, "appearance.pick_up", app.tr("cmd.appearance_pick_up")));
    if let Ok(target) = chain_target(app, &sel) {
        menu.children.push(cmd(app, "appearance.copy", app.tr("cmd.appearance_copy")).with(json!({"target":target})));
    }
    menu.children
        .push(cmd(app, "appearance.paste", app.tr("cmd.appearance_paste")).with(targets.clone()).on(app.session.appearance_clipboard.is_some()));
    menu.children.push(cmd(app, "appearance.assign", app.tr("render.remove_override")).with(targets));
    menu
}

/// Reuse the viewport's shared hover drawing, including modifier-specific drop size.
pub(crate) fn hover(app: &SolveApp) -> Option<&Sel> {
    app.render_workspace.view.assign.hover.as_ref()
}

pub(crate) fn context(app: &mut SolveApp, candidate: &(Hit, Sel)) {
    app.render_workspace.view.assign.context = Some(match &candidate.0 {
        Hit::Face { body, index, point } => Sel::Face { body: body.clone(), index: *index, point: *point },
        _ => candidate.1.clone(),
    });
}

pub(crate) fn assigned(app: &mut SolveApp, result: &Value) {
    if let Some(look) = result.get("appearance").filter(|look| !look.is_null()) {
        remember(app, look.clone());
        app.set_status(crate::i18n::format("render.look_applied", &[("name", look["name"].as_str().unwrap_or(""))]), false);
        app.status_undo = true;
    }
}

fn favorite_key(look: &Value) -> (String, String) {
    ("appearances".into(), super::library::appearance_id(look))
}
fn is_favorite(app: &SolveApp, look: &Value) -> bool {
    app.preferences.render_library_favorites.contains(&favorite_key(look))
}
pub(crate) fn favorite_applied(app: &mut SolveApp, reply: &Value) {
    let key = favorite_key(&reply["look"]);
    if reply["favorite"] == true {
        if app.preferences.render_library_favorites.len() < 256 {
            app.preferences.render_library_favorites.insert(key);
        }
    } else {
        app.preferences.render_library_favorites.remove(&key);
    }
    super::library::invalidate(app);
}

fn selection_name(app: &SolveApp, sel: &Sel) -> String {
    match sel {
        Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => name.clone(),
        Sel::Occurrence { path } => {
            path.last().and_then(|id| app.session.doc.occurrences.iter().find(|o| o.id == *id)).map(|o| o.name.clone()).unwrap_or_default()
        }
        _ => String::new(),
    }
}

pub(crate) fn hover_tooltip(app: &mut SolveApp, response: &egui::Response, candidate: Option<&(Hit, Sel)>) {
    let Some((_, sel)) = candidate else { return };
    let Ok(target) = chain_target(app, sel) else { return };
    let Ok(reply) = app.session.execute("appearance.pick_up", &json!({"target":target})) else { return };
    let tip =
        crate::i18n::format("render.hover_look", &[("object", &selection_name(app, sel)), ("look", reply["look"]["name"].as_str().unwrap_or(""))]);
    ui_kit::Container::tooltip(response, &tip);
}

pub(crate) fn selection_bar(app: &mut SolveApp, ui: &mut egui::Ui, free: egui::Rect) {
    crate::shell::register_panel(app, "render:selection", None, None);
    if super::result_active(app) || app.render_workspace.view.stage != super::Stage::Look {
        return;
    }
    let Some(sel) = app.session.selection.last().cloned() else { return };
    let Ok(target) = chain_target(app, &sel) else { return };
    let Some(bounds) = crate::viewport::selection_bounds(app) else { return };
    let Some(viewport) = app.viewport.rect else { return };
    let proj = crate::viewport::projection(app, viewport);
    let mut screen = egui::Rect::NOTHING;
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                if let Some(p) = proj.to_screen(solidcraft_engine::geom::Vec3::new(x, y, z)).filter(|p| p.is_finite()) {
                    screen.extend_with(p);
                }
            }
        }
    }
    if !screen.is_positive() || !screen.intersects(free) {
        return;
    }
    let Ok(reply) = app.session.execute("appearance.pick_up", &json!({"target":target})) else { return };
    let favorite = is_favorite(app, &reply["look"]);
    let labels = [app.tr("cmd.appearance_chain"), app.tr("render.replace_all"), app.tr("cmd.appearance_favorite"), app.tr("ui.hide")];
    let actions = ["chain", "reorder", "star", "suppress"]
        .iter()
        .zip(&labels)
        .enumerate()
        .map(|(index, (icon, label))| ui_kit::StageAction {
            icon: Some(icon),
            label,
            enabled: index != 2 || favorite || app.preferences.render_library_favorites.len() < 256,
            primary: index == 2 && favorite,
            reason: None,
        })
        .collect::<Vec<_>>();
    let anchor = egui::pos2(
        screen.center().x.clamp(free.left(), free.right()),
        (screen.top() - crate::theme::Tokens::get().space_12).clamp(free.top(), free.bottom()),
    );
    let ctx = ui.ctx().clone();
    let pill = ui_kit::Container::selection_card(&ctx, egui::Id::new("render_selection"), anchor, free, |ui| {
        ui_kit::selection_actions(ui, &selection_name(app, &sel), &actions)
    });
    crate::scenario::publish_rect("render:selection", pill.response.rect);
    crate::shell::register_panel(app, "render:selection", Some(pill.response.rect), None);
    match pill.inner {
        Some(0) => app.start("appearance.chain"),
        Some(1) => app.start("appearance.replace"),
        Some(2) => app.start("appearance.favorite"),
        Some(3) => {
            let params = match &sel {
                Sel::Occurrence { path } => json!({"paths":[path], "visible":false}),
                _ => json!({"items":[selection_name(app, &sel)], "visible":false}),
            };
            let _ = app.run("browser.visibility", params);
        }
        Some(4) => {
            let target = match &sel {
                Sel::Occurrence { path } => crate::context_menu::Target::Occurrence { path: path.clone() },
                _ => crate::context_menu::Target::Body { name: selection_name(app, &sel) },
            };
            crate::context_menu::open_for(app, pill.response.rect.left_bottom(), target);
        }
        _ => {}
    }
}

/// Existing appearance commands exposed alongside the shared inheritance widget.
pub(crate) fn selection_controls(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(sel) = app.session.selection.last().cloned() else { return };
    let Ok(target) = chain_target(app, &sel) else { return };
    if let Ok(chain) = app.session.execute("appearance.chain", &json!({"target":target})) {
        let steps = chain["steps"].as_array().map(Vec::as_slice).unwrap_or_default();
        let looks =
            std::array::from_fn(|index| steps.get(index).and_then(|s| s.get("look")).filter(|l| !l.is_null()).map(super::library::tile_sphere));
        let active = match chain["active"].as_str() {
            Some("face") => ui_kit::LookStep::Face,
            Some("body") => ui_kit::LookStep::Body,
            Some("component") => ui_kit::LookStep::Component,
            _ => ui_kit::LookStep::Material,
        };
        if ui_kit::look_chain(
            ui,
            "render:look_chain",
            [app.tr("render.pick_face"), app.tr("render.pick_body"), app.tr("render.pick_component"), app.tr("render.look_material")],
            looks,
            active,
        )
        .is_some()
        {
            app.start("appearance.assign");
        }
    }
    ui_kit::dropdown_menu(ui, "render_selection_actions", app.tr("cmd.appearance_assign"), |ui| {
        for (id, label) in
            [("appearance.assign", "cmd.appearance_assign"), ("appearance.replace", "render.replace_all"), ("appearance.copy", "cmd.appearance_copy")]
        {
            if let Some(command) = solidcraft_engine::find_command(id) {
                let info = command.info(&app.session);
                if ui_kit::command_menu_button(
                    ui,
                    id,
                    app.tr(label),
                    command.icon,
                    &crate::keymap::display_for(app, id).unwrap_or_default(),
                    info.enabled,
                )
                .clicked()
                {
                    app.start(id);
                    ui.close();
                }
            }
        }
        if ui_kit::command_menu_button(ui, "render_remove_override", app.tr("render.remove_override"), "delete", "", true).clicked() {
            if let Ok(targets) = targets(app, &app.session.selection) {
                let _ = app.run("appearance.assign", targets);
            }
            ui.close();
        }
    });
}

pub(crate) fn replacement_dialog(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending) = app.render_workspace.view.assign.replacement.clone() else { return };
    if pending.revision != app.session.revision || !pending.document.ptr_eq(&std::sync::Arc::downgrade(&app.session.doc)) {
        app.render_workspace.view.assign.replacement = None;
        return;
    }
    let from = pending.from;
    let entries = app.session.execute("appearance.library", &json!({})).ok().and_then(|v| v["appearances"].as_array().cloned()).unwrap_or_default();
    let entries: Vec<_> = entries.into_iter().filter(|entry| entry["available"] != false).collect();
    let mut selected = app.render_workspace.view.assign.replacement_index;
    let title = app.tr("render.replace_all").to_string();
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("render_replace_look"), "reorder", &title)
        .actions(&title, app.tr_label("Cancel"), None)
        .show(ctx, !entries.is_empty(), |ui| {
            ui_kit::text(ui, from["name"].as_str().unwrap_or(""), ui_kit::Text::Inline);
            let names: Vec<_> = entries.iter().map(|entry| entry["name"].as_str().unwrap_or("")).collect();
            ui_kit::choice_row(ui, app.tr("render.replace_with"), "render_replace_with", &mut selected, &names);
        });
    app.render_workspace.view.assign.replacement_index = selected;
    if action == ui_kit::Action::Ok {
        if let Some(to) = entries.get(selected) {
            let to = to["id"].as_str().or_else(|| to["name"].as_str()).map(|name| json!(name));
            if let Some(to) = to
                && let Err(error) = app.run("appearance.replace", json!({"from":from,"to":to}))
            {
                app.set_status(error, true);
            }
        }
        app.render_workspace.view.assign.replacement = None;
    } else if closed || action == ui_kit::Action::Cancel {
        app.render_workspace.view.assign.replacement = None;
    }
}

pub(crate) fn replacement_open(app: &SolveApp) -> bool {
    app.render_workspace.view.assign.replacement.is_some()
}
