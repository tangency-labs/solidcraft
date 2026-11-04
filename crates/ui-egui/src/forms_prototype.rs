//! Opt-in local Forms editing experiment, inside SolidCraft's normal viewport shell.
use crate::gpu::{Frame, GpuScene, SceneSlot, ViewportCallback};
use crate::{SolveApp, theme::Tokens, viewport::Proj};
use egui::{Pos2, Sense, Stroke};
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::geom::Mesh;
use solidcraft_engine::geom::prelude::*;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
mod combine_preview;
mod context;
mod controls;
mod curves;
mod extrusion;
mod finish;
mod interchange;
mod live_cad;
mod panel;
mod selection;
mod surface;
mod tool_panel;
mod tools;
pub(crate) use interchange::pick as pick_blend;
use surface::Element;

#[derive(Clone, Copy, Default, PartialEq)]
enum Pick {
    #[default]
    All,
    Vertex,
    Edge,
    Face,
}
#[derive(Clone, Copy, Default, PartialEq)]
enum Axis {
    #[default]
    View,
    X,
    Y,
    Z,
}
impl Axis {
    fn vector(self) -> Option<Vec3> {
        match self {
            Self::View => None,
            Self::X => Some(Vec3::X),
            Self::Y => Some(Vec3::Y),
            Self::Z => Some(Vec3::Z),
        }
    }
}

struct Drag {
    indices: Vec<usize>,
    points: Vec<Vec3>,
    start: Pos2,
    proj: Proj,
    anchor: Vec3,
    axis: Axis,
    moved: bool,
    basis: [Vec3; 3],
    scale: bool,
    length: f64,
    control: bool,
    extrude: bool,
    toggle: Option<Element>,
}
struct RepairEntry {
    feature: u64,
    done: std::sync::mpsc::Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
}

pub struct State {
    repair_offer: Option<(u64, String)>,
    repair_entry: Option<RepairEntry>,
    combination: combine_preview::State,
    show_combination: bool,
    context: context::State,
    curves: curves::State,
    extrusion: extrusion::State,
    other_preview: Option<u64>,
    matching_preview: bool,
    selected: BTreeSet<usize>,
    elements: BTreeSet<Element>,
    selection: selection::State,
    hover: Option<Element>,
    pick: Pick,
    axis: Axis,
    drag: Option<Drag>,
    gizmo_drag: Option<Drag>,
    cage: bool,
    points: bool,
    primitive: usize,
    wire: bool,
    orientation: usize,
    scale: bool,
    pivot: Option<Vec3>,
    setting_pivot: bool,
    controls: controls::Fields,
    pending: Option<finish::Pending>,
    interchange: Option<interchange::Pending>,
    blend_surface: Option<bool>,
    live: live_cad::State,
    tools: tools::State,
    tool_panel: Option<tool_panel::State>,
    slot: SceneSlot,
    // Sketch, face highlights and preview: only the middle slot has Form geometry.
    overlay_slots: [SceneSlot; 3],
    uploaded: u64,
    pub(crate) projected: Vec<(usize, Pos2)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            repair_offer: None,
            repair_entry: None,
            combination: Default::default(),
            show_combination: true,
            context: Default::default(),
            curves: Default::default(),
            extrusion: Default::default(),
            other_preview: None,
            matching_preview: false,
            selected: BTreeSet::new(),
            elements: BTreeSet::new(),
            selection: Default::default(),
            hover: None,
            pick: Pick::All,
            axis: Axis::View,
            drag: None,
            gizmo_drag: None,
            cage: false,
            points: true,
            primitive: 0,
            wire: false,
            orientation: 0,
            scale: false,
            pivot: None,
            setting_pivot: false,
            controls: Default::default(),
            pending: None,
            interchange: None,
            blend_surface: None,
            live: Default::default(),
            tools: Default::default(),
            tool_panel: None,
            slot: Arc::new(Mutex::new(None)),
            overlay_slots: std::array::from_fn(|_| Arc::new(Mutex::new(Some(GpuScene::default())))),
            uploaded: 0,
            projected: Vec::new(),
        }
    }
}

pub fn startup(app: &mut SolveApp) {
    if solidcraft_engine::geom::env::var_os("FORMS_PROTOTYPE").is_some() {
        match app.session.execute("form.prototype.create", &json!({})) {
            Ok(_) => opened(app),
            Err(e) => app.set_status(e.to_string(), true),
        }
    }
}
pub(crate) fn enter(app: &mut SolveApp, id: u64) {
    if app.session.active_sketch.is_some() {
        app.finish_sketch();
    }
    if run(app, "form.edit", json!({"feature":id})) {
        opened(app);
    } else if app.session.forms_prototype.is_none()
        && app.session.doc.feature(id).is_some_and(|feature| matches!(&feature.kind, solidcraft_engine::doc::FeatureKind::Form {definition,..} if definition.matching.is_some() || definition.profile.is_some()))
        && let Some((error, _, true)) = &app.status
    {
        app.forms_prototype.repair_offer = Some((id, error.clone()));
    }
}
pub(crate) fn start_source(app: &mut SolveApp, pipe: bool) {
    if app.session.active_sketch.is_some() {
        app.finish_sketch();
    }
    if app.session.forms_prototype.is_none() {
        let state = app.session.model.state();
        let available = if pipe {
            !state.sketches.is_empty() || state.bodies.iter().any(|b| !b.body.is_mesh() && b.body.edge_count() > 0)
        } else {
            state.bodies.iter().any(|b| !b.body.is_mesh() && !b.body.is_surface())
        };
        if !available {
            app.set_status(app.tr(if pipe { "forms.prototype.pipe_source_required" } else { "forms.prototype.surface_required" }).to_owned(), true);
            return;
        }
        if !run(app, "form.prototype.create", json!({})) {
            return;
        }
        opened(app);
    }
    tool_panel::start(app, if pipe { tool_panel::Mode::Pipe } else { tool_panel::Mode::Surface });
}
pub(crate) fn opened(app: &mut SolveApp) {
    if app.session.forms_prototype.is_some() {
        app.forms_prototype = State::default();
        app.forms_prototype.controls.tolerance = format!("{} mm", app.preferences.forms_cad_tolerance);
        controls::sync(app);
        panel::sync(app);
        controls::load(app);
        app.home.open = false;
        fit(app);
    }
}
pub fn inspect(app: &SolveApp) -> serde_json::Value {
    let projection = app.viewport.rect.map(|rect| crate::viewport::projection(app, rect));
    let edges: Vec<_> = app
        .session
        .forms_prototype
        .iter()
        .flat_map(|f| f.surface.boundaries.iter().enumerate())
        .filter_map(|(i, (edge, initial))| {
            let curve = surface::boundary_curve(app, i, initial);
            let proj = projection.as_ref()?;
            let p = if curves::exact_picking(app) { curves::get(app, *edge)?.at(0.5).ok()? } else { *curve.get(curve.len() / 2)? };
            if !surface::visible(app, proj, p) {
                return None;
            }
            let screen = proj.to_screen(p)?;
            Some(json!({"vertices":edge,"screen":[screen.x,screen.y]}))
        })
        .collect();
    let elements: Vec<_> = app
        .forms_prototype
        .elements
        .iter()
        .map(|e| match e {
            Element::Vertex(v) => json!({"kind":"vertex","vertex":v}),
            Element::Edge(e) => json!({"kind":"edge","vertices":e}),
            Element::Face(f) => json!({"kind":"face","face":f}),
            Element::Surface => json!({"kind":"surface"}),
        })
        .collect();
    json!({"active":app.session.forms_prototype.is_some(),"repair_offer":app.forms_prototype.repair_offer,"repair_pending":app.forms_prototype.repair_entry.is_some(),"selected":app.forms_prototype.selected,"elements":elements,
        "edges":edges,"extrusion":extrusion::inspect(app),
        "display":live_cad::inspect(app),"source_curves":curves::inspect(app),"context":context::inspect(app),"tool":tool_panel::inspect(app),
        "combination":{"ready":combine_preview::ready(app).is_some(),"busy":combine_preview::busy(app),
            "error":combine_preview::result(app).and_then(|r|r.as_ref().err())},
        "vertices":app.forms_prototype.projected.iter().map(|(i,p)|json!({"index":i,"screen":[p.x,p.y]})).collect::<Vec<_>>()})
}
fn run(app: &mut SolveApp, id: &str, p: serde_json::Value) -> bool {
    match app.session.execute(id, &p) {
        Ok(_) => {
            command_completed(app, id);
            app.status = None;
            true
        }
        Err(e) => {
            app.set_status(e.to_string(), true);
            false
        }
    }
}
pub(crate) fn command_completed(app: &mut SolveApp, id: &str) {
    if id == "form.prototype.finish_options" {
        controls::load(app);
    }
    if matches!(id, "form.prototype.close" | "form.prototype.commit") {
        app.forms_prototype = State::default();
    }
    if id == "browser.visibility" {
        app.forms_prototype.context = Default::default();
    }
    if matches!(id, "form.prototype.parameters" | "form.prototype.undo" | "form.prototype.redo") {
        controls::sync(app);
        panel::sync(app);
    }
    if matches!(
        id,
        "form.prototype.topology"
            | "form.prototype.unweld"
            | "form.prototype.weld"
            | "form.prototype.insert_edge"
            | "form.prototype.insert_loop"
            | "form.prototype.insert_points"
            | "form.prototype.refine"
            | "form.prototype.undo"
            | "form.prototype.redo"
            | "form.prototype.parameters"
            | "form.surface"
            | "form.pipe"
            | "form.blend.import"
    ) {
        app.forms_prototype.elements.clear();
        app.forms_prototype.selected.clear();
        app.forms_prototype.pivot = None;
        app.forms_prototype.selection = Default::default();
        app.forms_prototype.hover = None;
    }
}
pub(crate) fn fit(app: &mut SolveApp) {
    if let Some(f) = &app.session.forms_prototype {
        let points = f.control_positions();
        let center = points.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / points.len().max(1) as f64;
        let bounds: f64 = points.iter().map(|p| (*p - center).len()).fold(0.01, f64::max);
        app.cancel_view_animation();
        app.pending_fit = None;
        app.cam = solidcraft_engine::render::Camera { target: center, distance: bounds * 4.0, ..Default::default() };
        // An explicit Form fit also fulfils the initial application fit. Otherwise
        // an import completed before first layout is replaced by the empty model's view.
        app.fitted = true;
    }
}

pub(crate) fn leave_document(app: &mut SolveApp) {
    extrusion::suspend(app);
}
pub(crate) fn resume_document(app: &mut SolveApp) {
    // GPU callbacks consume these upload slots. Another tab replaces the shared
    // renderer's batches, even though this tab's authored Form has not changed.
    app.forms_prototype.uploaded = 0;
    for slot in &app.forms_prototype.overlay_slots {
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(GpuScene::default());
    }
}
pub(crate) fn background_tick(app: &mut SolveApp, ctx: &egui::Context) {
    repair_entry_tick(app);
    interchange::tick(app, ctx);
    extrusion::tick(app, ctx);
}

pub(crate) fn repair_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((feature, error)) = app.forms_prototype.repair_offer.clone() else { return };
    let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::new("forms_repair_offer"), "edit", app.tr("forms.repair.title"))
        .actions(app.tr("forms.repair.open"), app.tr("forms.prototype.cancel"), None)
        .show(ctx, app.forms_prototype.repair_entry.is_none(), |ui| {
            ui.add(egui::Label::new(app.tr("forms.repair.offer")).wrap());
            if app.forms_prototype.repair_entry.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(app.tr("forms.prototype.previewing"));
                });
            }
            ui.add(egui::Label::new(egui::RichText::new(&error).color(Tokens::get().error)).wrap());
        });
    if action == crate::ui_kit::Action::Ok {
        start_repair_entry(app, ctx, feature);
    } else if closed || action == crate::ui_kit::Action::Cancel {
        app.forms_prototype.repair_entry = None;
        app.forms_prototype.repair_offer = None;
    }
}
fn start_repair_entry(app: &mut SolveApp, ctx: &egui::Context, feature: u64) {
    if app.forms_prototype.repair_entry.is_some() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let copy = app.session.scratch();
        let wake = ctx.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        match std::thread::Builder::new().name("form-support-repair".into()).spawn(move || {
            let result = copy.compute_commands(&[("form.repair".into(), json!({"feature":feature}))], None).map_err(|e| e.to_string());
            let _ = tx.send(result);
            wake.request_repaint();
        }) {
            Ok(_) => app.forms_prototype.repair_entry = Some(RepairEntry { feature, done: rx }),
            Err(error) => repair_entry_error(app, feature, error.to_string()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        if run(app, "form.repair", json!({"feature":feature})) {
            opened(app);
        } else if let Some((error, _, true)) = app.status.clone() {
            repair_entry_error(app, feature, error);
        }
    }
}
fn repair_entry_tick(app: &mut SolveApp) {
    let Some(pending) = app.forms_prototype.repair_entry.as_ref() else { return };
    if !app.forms_prototype.repair_offer.as_ref().is_some_and(|(feature, _)| *feature == pending.feature) {
        app.forms_prototype.repair_entry = None;
        return;
    }
    let feature = pending.feature;
    let result = match pending.done.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(_) => Err(app.tr("forms.prototype.preview_stopped").to_owned()),
    };
    app.forms_prototype.repair_entry = None;
    // ComputedCommands also checks exact document/draft/authoring context.
    // A changed source or newly opened draft must take precedence over this job.
    match result.and_then(|prepared| app.session.accept_commands(prepared).map_err(|e| e.to_string())) {
        Ok(replies) => {
            for (id, reply) in replies {
                let _ = app.command_completed(&id, Ok(reply));
            }
            app.status = None;
        }
        Err(error) => repair_entry_error(app, feature, error),
    }
}
fn repair_entry_error(app: &mut SolveApp, feature: u64, error: String) {
    if let Some((offered, visible)) = app.forms_prototype.repair_offer.as_mut()
        && *offered == feature
    {
        *visible = error.clone();
    }
    app.set_status(error, true);
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    finish::tick(app);
    if app.session.forms_prototype.is_none() {
        app.forms_prototype.tool_panel = None;
        app.forms_prototype.matching_preview = false;
        return;
    }
    tool_panel::tick(app);
    live_cad::tick(app, ui.ctx());
    if !app.session.forms_prototype.as_ref().is_some_and(|f| f.repair.is_some()) {
        curves::tick(app, ui.ctx());
        combine_preview::tick(app, ui.ctx());
    }
    context::tick(app, ui.ctx());
    panel::show(app, ui);
    live_cad::restore_fixed_profile_handles(app);
    if app.session.forms_prototype.is_none() {
        return;
    }
    let busy = app.forms_prototype.pending.is_some()
        || app.forms_prototype.interchange.is_some()
        || extrusion::released(app)
        || app.session.forms_prototype.as_ref().is_some_and(|f| f.repair.is_some());
    let token = Tokens::get();
    let rect = ui.available_rect_before_wrap();
    // Keep all viewport painting, including tool previews and the CPU renderer,
    // inside the space left by the panel without changing the panel's own clip.
    let mut viewport_ui = ui.new_child(egui::UiBuilder::new().id_salt("forms_viewport").max_rect(rect));
    viewport_ui.set_clip_rect(ui.clip_rect().intersect(rect));
    let ui = &mut viewport_ui;
    app.viewport.rect = Some(rect);
    crate::viewport::refresh_layout(app, ui.ctx());
    let before = crate::viewport::projection(app, rect);
    let geometry_first = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| matches!(app.forms_prototype.pick, Pick::Vertex | Pick::Edge) && surface::nearest(app, &before, p).is_some());
    let insertion_active = tools::active(app);
    let tool_active = insertion_active || tool_panel::active(app);
    let gizmo_hot = !geometry_first
        && !tool_active
        && ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| triad(app).is_some_and(|t| crate::gizmo::contains_oriented(&before, &t, p, basis(app))));
    let painter = ui.painter().clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("forms_transform")));
    let dragging_gizmo = if busy || tool_active {
        false
    } else if geometry_first && app.forms_prototype.gizmo_drag.is_none() {
        ui.add_enabled_ui(false, |ui| {
            show_gizmo(app, ui, &painter, &before);
        });
        false
    } else {
        show_gizmo(app, ui, &painter, &before)
    };
    let response = ui.interact(rect, ui.id().with("forms_prototype_view"), Sense::click());
    let input = crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps::MODEL);
    let proj = crate::viewport::projection(app, rect);
    if input.primary_pressed
        && input.inside
        && input.mode.is_none()
        && !gizmo_hot
        && !dragging_gizmo
        && !busy
        && !tool_active
        && let Some(pos) = input.hover
    {
        begin_drag(app, &proj, pos, input.mods);
    }
    if let Some(pos) = input.hover
        && input.primary_down
        && input.mode.is_none()
        && !dragging_gizmo
        && !busy
        && !tool_active
    {
        if input.mods.shift
            && app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_none())
            && app.forms_prototype.elements.iter().any(|e| matches!(e, Element::Face(_) | Element::Edge(_)))
            && let Some(d) = app.forms_prototype.drag.as_mut()
            && !d.moved
        {
            d.extrude = true;
        }
        pull(app, pos);
    }
    if !input.primary_down
        && let Some(d) = app.forms_prototype.drag.take()
    {
        if d.extrude && d.moved && !extrusion::release(app) {
            select_extruded_edges(app);
        } else if !d.moved {
            selection::released(app, d.toggle);
        }
    }
    if input.inside
        && !tool_active
        && !busy
        && ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary))
        && let Some(Element::Edge(edge)) = input.hover.and_then(|p| surface::nearest(app, &proj, p))
    {
        app.forms_prototype.elements.insert(Element::Edge(edge));
        selection::expand(app, false);
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        tool_panel::cancel(app);
        let cancelling_extrusion = extrusion::cancel(app);
        let drag = app.forms_prototype.drag.take().or_else(|| app.forms_prototype.gizmo_drag.take());
        crate::gizmo::cancel(ui.ctx(), ui.id().with("forms_transform_drag"));
        if !cancelling_extrusion && drag.is_some_and(|d| d.moved) {
            run(app, "form.prototype.undo", json!({}));
        }
        app.forms_prototype.setting_pivot = false;
    }
    live_cad::restore_fixed_profile_handles(app);
    ui.painter().rect_filled(rect, 0.0, token.viewport_top);
    let hover_blocked = busy || tool_active || gizmo_hot || dragging_gizmo || app.forms_prototype.drag.is_some() || extrusion::active(app);
    app.forms_prototype.hover = input.hover.filter(|_| input.inside && !hover_blocked).and_then(|p| surface::nearest(app, &proj, p));
    render(app, ui, &proj);
    surface::overlay(app, ui, &proj);
    tool_panel::pointer(app, ui, &proj, &input);
    if insertion_active && !busy {
        tools::pointer(app, ui, &proj, &input);
    }
    crate::viewport::axis_view(app, ui, rect);
}
fn select_all(app: &mut SolveApp) {
    app.forms_prototype.pivot = None;
    if let Some(form) = &app.session.forms_prototype {
        app.forms_prototype.elements = match app.forms_prototype.pick {
            Pick::Vertex => {
                form.logical.faces.iter().flatten().copied().filter(|&id| live_cad::handle_available(app, id)).map(Element::Vertex).collect()
            }
            Pick::Edge => form.surface.boundaries.iter().map(|(e, _)| Element::Edge(*e)).collect(),
            Pick::Face | Pick::All if form.profile.is_some() => [Element::Surface].into_iter().collect(),
            Pick::Face | Pick::All => form.logical.faces.iter().enumerate().filter(|(_, f)| !f.is_empty()).map(|(i, _)| Element::Face(i)).collect(),
        };
    }
    selection::sync(app);
}
/// Context shortcuts must never delete document bodies while a cage is being edited.
pub(crate) fn shortcut(app: &mut SolveApp, key: egui::Key, mods: egui::Modifiers) -> bool {
    if app.session.forms_prototype.is_none() {
        return false;
    }
    if (tools::active(app) || tool_panel::active(app))
        && (matches!(key, egui::Key::Delete | egui::Key::Backspace) || (mods.command && matches!(key, egui::Key::A | egui::Key::Z | egui::Key::Y)))
    {
        return true;
    }
    if mods.command && !mods.alt && !mods.shift && key == egui::Key::A {
        select_all(app);
        return true;
    }
    if mods.is_none() && matches!(key, egui::Key::Delete | egui::Key::Backspace) {
        let faces: Vec<_> = surface::selected_faces(app).into_iter().collect();
        let edges: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Edge(e) = e { Some(*e) } else { None }).collect();
        let tool = if !faces.is_empty() {
            Some("delete_faces")
        } else if !edges.is_empty() {
            Some("dissolve")
        } else {
            None
        };
        if let Some(tool) = tool
            && run(app, "form.prototype.topology", json!({"tool":tool,"faces":faces,"edges":edges}))
        {
            app.forms_prototype.elements.clear();
            app.forms_prototype.selected.clear();
            app.forms_prototype.pivot = None;
            app.forms_prototype.selection = Default::default();
        }
        return true;
    }
    false
}

fn begin_drag(app: &mut SolveApp, proj: &Proj, pos: Pos2, mods: egui::Modifiers) {
    let add = mods.shift || mods.ctrl || mods.command || app.preferences.forms_greedy_selection;
    let Some(hit) = surface::nearest(app, proj, pos) else {
        if !(mods.shift || mods.ctrl || mods.command) {
            app.forms_prototype.selected.clear();
            app.forms_prototype.elements.clear();
            app.forms_prototype.selection = Default::default();
            app.forms_prototype.pivot = None;
        }
        return;
    };
    let Some(form) = &app.session.forms_prototype else { return };
    if app.forms_prototype.setting_pivot {
        let points: Vec<_> = hit.vertices(&form.logical).iter().filter_map(|&i| form.surface.vertices.get(i).copied()).collect();
        if !points.is_empty() {
            app.forms_prototype.pivot = Some(points.iter().copied().fold(Vec3::ZERO, |a, b| a + b) / points.len() as f64);
        }
        app.forms_prototype.setting_pivot = false;
        return;
    }
    let previous = app.forms_prototype.elements.clone();
    if !add && !app.forms_prototype.elements.contains(&hit) {
        app.forms_prototype.elements.clear();
    }
    let toggle = (add && app.forms_prototype.elements.contains(&hit)).then_some(hit);
    if toggle.is_none() {
        app.forms_prototype.elements.insert(hit);
    }
    app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&form.logical)).collect();
    if app.forms_prototype.selected.is_empty() {
        app.forms_prototype.drag = None;
        return;
    }
    if previous != app.forms_prototype.elements {
        app.forms_prototype.pivot = None;
    }
    let indices: Vec<_> = app.forms_prototype.selected.iter().copied().collect();
    let skin: Vec<_> = indices.iter().filter_map(|&i| form.surface.vertices.get(i).copied()).collect();
    let anchor = skin.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / skin.len().max(1) as f64;
    let control = form.hierarchy.is_none() && indices.len() == form.cage.points.len();
    let points = if control { form.control_positions() } else { skin };
    app.forms_prototype.drag = Some(Drag {
        indices,
        points,
        start: pos,
        proj: *proj,
        anchor,
        axis: app.forms_prototype.axis,
        moved: false,
        basis: basis(app),
        scale: false,
        length: 1.0,
        control,
        extrude: form.profile.is_none() && mods.shift && matches!(hit, Element::Face(_) | Element::Edge(_)),
        toggle,
    });
    selection::pressed(app, hit, mods.shift);
}
fn basis(app: &SolveApp) -> [Vec3; 3] {
    if let Some(d) = &app.forms_prototype.gizmo_drag {
        return d.basis;
    }
    if app.forms_prototype.orientation == 1 {
        let (x, y, z) = app.cam.basis();
        return [x, y, z];
    }
    if app.forms_prototype.orientation == 2
        && let Some(form) = &app.session.forms_prototype
    {
        let ids: Vec<_> = app.forms_prototype.selected.iter().copied().collect();
        let z = selection_normal(form, &ids).unwrap_or(Vec3::Z);
        let edge = ids.windows(2).find_map(|p| Some(*form.surface.vertices.get(p[1])? - *form.surface.vertices.get(p[0])?)).unwrap_or(Vec3::X);
        let x = (edge - z * z.dot(edge)).normalized().unwrap_or_else(|| z.any_perp().normalized().unwrap_or(Vec3::X));
        return [x, z.cross(x), z];
    }
    [Vec3::X, Vec3::Y, Vec3::Z]
}
fn selection_normal(form: &solidcraft_engine::forms_prototype::Form, ids: &[usize]) -> Option<Vec3> {
    if ids.iter().any(|&id| id >= form.cage.points.len()) {
        let face = form.logical.faces.last()?;
        let a = *form.surface.vertices.get(*face.first()?)?;
        let b = *form.surface.vertices.get(*face.get(1)?)?;
        let c = *form.surface.vertices.get(*face.get(2)?)?;
        return (b - a).cross(c - a).normalized();
    }
    ids.iter().filter_map(|&id| form.surface.mesh.normals.get(id)).copied().fold(Vec3::ZERO, |a, b| a + b).normalized()
}
fn triad(app: &SolveApp) -> Option<crate::gizmo::Triad> {
    if app.forms_prototype.selected.iter().any(|&id| !live_cad::handle_available(app, id)) {
        return None;
    }
    let form = app.session.forms_prototype.as_ref()?;
    let points: Vec<_> = app.forms_prototype.selected.iter().filter_map(|&i| form.surface.vertices.get(i).copied()).collect();
    if points.is_empty() {
        return None;
    }
    let center = app.forms_prototype.pivot.unwrap_or_else(|| points.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / points.len() as f64);
    Some(crate::gizmo::Triad { center, translate: true, rotate: !app.forms_prototype.scale })
}
pub(crate) fn owns_pointer(app: &SolveApp, proj: &Proj, pos: Pos2) -> bool {
    app.session.forms_prototype.is_some()
        && (surface::nearest(app, proj, pos).is_some() || triad(app).is_some_and(|t| crate::gizmo::contains_oriented(proj, &t, pos, basis(app))))
}
pub(crate) fn owns_key(app: &SolveApp, key: egui::Key) -> bool {
    app.session.forms_prototype.is_some()
        && (tools::active(app) || tool_panel::active(app) || (extrusion::active(app) && key == egui::Key::Escape))
        && matches!(key, egui::Key::M | egui::Key::Enter | egui::Key::Escape)
}
pub(crate) fn shift_dragging(app: &SolveApp) -> bool {
    app.session.forms_prototype.is_some()
        && (app.forms_prototype.drag.as_ref().is_some_and(|d| d.extrude) || app.forms_prototype.gizmo_drag.as_ref().is_some_and(|d| d.extrude))
}
fn show_gizmo(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) -> bool {
    let Some(t) = triad(app) else { return false };
    let id = ui.id().with("forms_transform_drag");
    let dirs = basis(app);
    let shift_extrude = app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_none())
        && ui.input(|i| i.modifiers.shift)
        && app.forms_prototype.elements.iter().any(|e| matches!(e, Element::Face(_) | Element::Edge(_)));
    let scale = app.forms_prototype.scale && !shift_extrude;
    let Some(event) = crate::gizmo::show_mode(ui, painter, proj, id, &t, 0.0, (dirs, scale)) else {
        return app.forms_prototype.gizmo_drag.is_some();
    };
    if event.started || app.forms_prototype.gizmo_drag.is_none() {
        let Some(form) = &app.session.forms_prototype else { return false };
        let indices: Vec<_> = app.forms_prototype.selected.iter().copied().collect();
        let control = form.hierarchy.is_none() && indices.len() == form.cage.points.len();
        let points = if control { form.control_positions() } else { indices.iter().filter_map(|&i| form.surface.vertices.get(i).copied()).collect() };
        app.forms_prototype.drag = None;
        let (right, _, _) = app.cam.basis();
        let pixels = proj.to_screen(t.center).zip(proj.to_screen(t.center + right)).map_or(1.0, |(a, b)| a.distance(b).max(1e-6));
        app.forms_prototype.gizmo_drag = Some(Drag {
            indices,
            points,
            start: Pos2::ZERO,
            proj: *proj,
            anchor: t.center,
            axis: Axis::View,
            moved: false,
            basis: dirs,
            scale,
            length: crate::gizmo::LEN_PX / f64::from(pixels),
            control,
            extrude: shift_extrude,
            toggle: None,
        });
    }
    if shift_extrude
        && let Some(d) = app.forms_prototype.gizmo_drag.as_mut()
        && !d.moved
    {
        d.extrude = true;
        d.scale = false;
    }
    if let Some(d) = &app.forms_prototype.gizmo_drag {
        let positions: Vec<[f64; 3]> = d
            .points
            .iter()
            .map(|&p| {
                let q = match event.change {
                    crate::gizmo::Change::Translate(delta) if d.scale => {
                        let local = p - d.anchor;
                        let factors = d.basis.map(|a| (1.0 + delta.dot(a) / d.length).max(0.001));
                        let factors = if event.handle == crate::gizmo::Handle::Ball {
                            let (r, u, _) = app.cam.basis();
                            let f = (1.0 + delta.dot(r + u) / d.length).max(0.001);
                            [f; 3]
                        } else {
                            factors
                        };
                        d.anchor + d.basis.iter().zip(factors).fold(Vec3::ZERO, |v, (&a, s)| v + a * (local.dot(a) * s))
                    }
                    crate::gizmo::Change::Translate(delta) => p + delta,
                    crate::gizmo::Change::Rotate { axis, angle } => {
                        let direction = d.basis[axis.min(2)];
                        d.anchor + crate::gizmo::apply(&crate::gizmo::rotation(direction, angle), p - d.anchor)
                    }
                };
                [q.x, q.y, q.z]
            })
            .collect();
        let changed = app.session.forms_prototype.as_ref().is_some_and(|f| {
            let controls;
            let points = if d.control {
                controls = f.control_positions();
                &controls
            } else {
                &f.surface.vertices
            };
            d.indices.iter().zip(&positions).any(|(&i, p)| points.get(i).is_some_and(|q| q.dist(Vec3::new(p[0], p[1], p[2])) > 1e-9))
        });
        let command = if d.control { "form.prototype.set_vertices" } else { "form.prototype.set_surface_vertices" };
        let extrude = d.extrude && matches!(event.change, crate::gizmo::Change::Translate(_));
        let params = if extrude {
            let crate::gizmo::Change::Translate(delta) = event.change else { return true };
            extrusion_params(app, delta, !d.moved)
        } else {
            json!({"vertices":d.indices,"positions":positions,"checkpoint":!d.moved})
        };
        if changed && (if extrude { extrusion::command(app, params) } else { run(app, command, params) }) {
            if let Some(d) = app.forms_prototype.gizmo_drag.as_mut() {
                d.moved = true;
            }
            if extrude
                && let Some(form) = &app.session.forms_prototype
                && form.hierarchy.is_some()
            {
                app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&form.logical)).collect();
            }
        }
    }
    if event.done
        && let Some(d) = app.forms_prototype.gizmo_drag.take()
        && d.extrude
        && d.moved
        && !extrusion::release(app)
    {
        select_extruded_edges(app);
    }
    true
}
fn displacement(d: &Drag, pos: Pos2) -> Option<Vec3> {
    if let Some(axis) = d.axis.vector() {
        let a = d.proj.to_screen(d.anchor)?;
        let b = d.proj.to_screen(d.anchor + axis * 10.0)?;
        let screen = b - a;
        if screen.length_sq() < 1.0 {
            return None;
        }
        Some(axis * f64::from((pos - d.start).dot(screen) / screen.length_sq()) * 10.0)
    } else {
        let hit = |at| {
            let (o, ray) = d.proj.ray(at);
            let normal = d.proj.cam.back();
            let den = ray.dot(normal);
            (den.abs() > 1e-9).then(|| o + ray * ((d.anchor - o).dot(normal) / den))
        };
        Some(hit(pos)? - hit(d.start)?)
    }
}
fn pull(app: &mut SolveApp, pos: Pos2) {
    let Some(d) = app.forms_prototype.drag.as_ref() else { return };
    if pos.distance(d.start) < 2.0 && !d.moved {
        return;
    }
    let Some(delta) = displacement(d, pos) else { return };
    if d.extrude {
        let params = extrusion_params(app, delta, !d.moved);
        if extrusion::command(app, params) {
            if let Some(d) = app.forms_prototype.drag.as_mut() {
                d.moved = true;
            }
            if let Some(f) = &app.session.forms_prototype {
                app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&f.logical)).collect();
            }
        }
        return;
    }
    let positions: Vec<[f64; 3]> = d
        .points
        .iter()
        .map(|p| {
            let q = *p + delta;
            [q.x, q.y, q.z]
        })
        .collect();
    let params = json!({"vertices":d.indices,"positions":positions,"checkpoint":!d.moved});
    let command = if d.control { "form.prototype.set_vertices" } else { "form.prototype.set_surface_vertices" };
    if run(app, command, params)
        && let Some(d) = app.forms_prototype.drag.as_mut()
    {
        d.moved = true;
    }
}
fn select_extruded_edges(app: &mut SolveApp) {
    let Some(form) = &app.session.forms_prototype else { return };
    let edges = form.extruded_edges();
    if edges.is_empty() {
        return;
    }
    app.forms_prototype.elements = edges.iter().copied().map(Element::Edge).collect();
    app.forms_prototype.selected = edges.iter().flatten().copied().collect();
    app.forms_prototype.selection = Default::default();
    app.forms_prototype.pivot = None;
}
fn extrusion_params(app: &SolveApp, delta: Vec3, checkpoint: bool) -> serde_json::Value {
    let faces: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).collect();
    let edges: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Edge(e) = e { Some(*e) } else { None }).collect();
    json!({"faces":faces,"edges":edges,"displacement":[delta.x,delta.y,delta.z],"checkpoint":checkpoint})
}

fn render(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj) {
    let Some(form) = &app.session.forms_prototype else { return };
    let ghost = extrusion::preview(app);
    let combined = combine_preview::ready(app).is_some();
    if app.viewport.gpu.is_none() {
        for entry in context::render_entries(app) {
            fallback(ui, proj, &entry.mesh);
        }
        if !combined {
            fallback(ui, proj, ghost.map(|p| &p.mesh).or_else(|| live_cad::mesh(app)).unwrap_or(&form.surface.mesh));
            extrusion::scene_submitted(app, proj);
        }
        return;
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    form.levels.hash(&mut hash);
    app.forms_prototype.context.version.hash(&mut hash);
    context::key(app).hash(&mut hash);
    controls::operation(app).hash(&mut hash);
    combined.hash(&mut hash);
    form.revision.hash(&mut hash);
    extrusion::preview_revision(app).hash(&mut hash);
    live_cad::mesh(app).is_some().hash(&mut hash);
    form.cage.faces.hash(&mut hash);
    Tokens::get().body.to_array().hash(&mut hash);
    app.forms_prototype.elements.hash(&mut hash);
    app.forms_prototype.hover.hash(&mut hash);
    app.forms_prototype.wire.hash(&mut hash);
    for p in &form.cage.points {
        p.x.to_bits().hash(&mut hash);
        p.y.to_bits().hash(&mut hash);
        p.z.to_bits().hash(&mut hash);
    }
    let key = hash.finish();
    if app.forms_prototype.uploaded != key {
        let mut scene = GpuScene::default();
        let mut highlight = GpuScene::default();
        context::append(app, &mut scene);
        let tokens = Tokens::get();
        let color = scene.color(
            match controls::operation(app) {
                1 => tokens.preview_add,
                2 | 3 => tokens.preview_cut,
                _ => tokens.body,
            }
            .to_array(),
        );
        let selected = highlight.color(Tokens::get().sel_face.to_array());
        let hovered = highlight.color(Tokens::get().body_selected.to_array());
        let faces = surface::selected_faces(app);
        let mesh = ghost.map(|p| &p.mesh).or_else(|| live_cad::mesh(app)).unwrap_or(&form.surface.mesh);
        for (ti, triangle) in mesh.triangles.iter().enumerate().filter(|_| !combined) {
            let root = mesh.tri_face.get(ti).copied().unwrap_or(u32::MAX) as usize;
            let c = if faces.contains(&root) {
                Some(selected)
            } else if app.forms_prototype.hover == Some(Element::Face(root))
                || (form.profile.is_some() && app.forms_prototype.hover == Some(Element::Surface))
            {
                Some(hovered)
            } else {
                None
            };
            for &i in triangle {
                if let (Some(p), Some(n)) = (mesh.positions.get(i as usize), mesh.normals.get(i as usize)) {
                    scene.tri(p.to_f32(), n.to_f32(), color);
                    if let Some(c) = c {
                        // The existing highlight pass tests biased depth without
                        // writing it; coincident support CAD cannot speckle this tint.
                        highlight.tri(p.to_f32(), n.to_f32(), c);
                    }
                }
            }
        }
        let edge_color = scene.color(Tokens::get().body_edge.to_array());
        if ghost.is_none() && live_cad::mesh(app).is_some() {
            for (i, curve) in mesh.edges.iter().enumerate() {
                if !mesh.seams.get(i).copied().unwrap_or(false) {
                    for pair in curve.windows(2) {
                        scene.line(pair[0].to_f32(), pair[1].to_f32(), edge_color, 1.4, false);
                    }
                }
            }
        } else {
            for (_, curve) in ghost.map_or(&form.surface.boundaries, |p| &p.boundaries) {
                for pair in curve.windows(2) {
                    scene.line(pair[0].to_f32(), pair[1].to_f32(), edge_color, 1.4, false);
                }
            }
        }
        if app.forms_prototype.wire {
            let wire_mesh = ghost.map_or(&form.surface.mesh, |p| &p.mesh);
            for t in &wire_mesh.triangles {
                for [a, b] in [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]] {
                    if let (Some(a), Some(b)) = (wire_mesh.positions.get(a as usize), wire_mesh.positions.get(b as usize)) {
                        scene.line(a.to_f32(), b.to_f32(), edge_color, 0.6, false);
                    }
                }
            }
        }
        *app.forms_prototype.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(scene);
        *app.forms_prototype.overlay_slots[1].lock().unwrap_or_else(|e| e.into_inner()) = Some(highlight);
        app.forms_prototype.uploaded = key;
    }
    let frame = viewport_frame(app, proj, ui.ctx().pixels_per_point(), key);
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(proj.rect, ViewportCallback(frame)));
    if !combined {
        extrusion::scene_submitted(app, proj);
    }
}
fn viewport_frame(app: &SolveApp, proj: &Proj, ppp: f32, key: u64) -> Frame {
    let slot = app.forms_prototype.slot.clone();
    let [sketch, highlight, preview] = app.forms_prototype.overlay_slots.clone();
    let mut empty_hash = std::collections::hash_map::DefaultHasher::new();
    "solidcraft-forms-empty-overlays".hash(&mut empty_hash);
    let empty_key = empty_hash.finish();
    let eye = app.cam.eye();
    let rect = proj.rect;

    Frame {
        product_lights: None,
        product: false,
        upload_error: None,
        key,
        slot: slot.clone(),
        sketch_key: empty_key,
        sketch_slot: sketch,
        hl_key: key,
        hl_slot: highlight,
        pv_key: empty_key,
        pv_slot: preview,
        pv_hide: false,
        preview_bounds: None,
        view_proj: proj.vp.to_f32(),
        back: app.cam.back().to_f32(),
        size_px: [rect.width() * ppp, rect.height() * ppp],
        clip: [0.0; 4],
        cap: [0.0; 4],
        images: Vec::new(),
        clouds: Vec::new(),
        analysis: [0.0; 8],
        analysis_colors: [0.0; 36],
        access: None,
        outline: false,
        eye: [eye.x as f32, eye.y as f32, eye.z as f32, if app.cam.fov > 0.0 { 1.0 } else { 0.0 }],
        ao_radius: 0.0,
        ao_samples: solidcraft_gpu::view::AO_SAMPLES,
        ssaa: 1.0,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        hatch: Default::default(),
        sky: crate::viewport::sky_uniform(app.preferences.dark_parts_shading, proj.cam.up),
    }
}
fn fallback(ui: &egui::Ui, proj: &Proj, mesh: &Mesh) {
    let mut triangles: Vec<_> = mesh
        .triangles
        .iter()
        .filter_map(|t| {
            let p = [*mesh.positions.get(t[0] as usize)?, *mesh.positions.get(t[1] as usize)?, *mesh.positions.get(t[2] as usize)?];
            Some((((p[0] + p[1] + p[2]) / 3.0 - proj.cam.eye()).len2(), p))
        })
        .collect();
    triangles.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, p) in triangles {
        if let [Some(a), Some(b), Some(c)] = p.map(|p| proj.to_screen(p)) {
            let mut triangle = egui::Mesh::default();
            for point in [a, b, c] {
                triangle.colored_vertex(point, Tokens::get().body);
            }
            triangle.add_triangle(0, 1, 2);
            ui.painter().add(egui::Shape::mesh(triangle));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_saved_surface_edit_offers_explicit_retained_repair_and_blocks_finish() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.session.execute("solid.box", &json!({"length":4,"width":6,"height":1,"body_name":"Support","operation":"new"})).unwrap();
        let body = h.app.session.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        h.app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
        let feature = h.app.session.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let solidcraft_engine::doc::FeatureKind::Form { definition, .. } = &mut h.app.session.doc_mut().feature_mut(feature).unwrap().kind else {
            panic!("Form")
        };
        let solidcraft_engine::doc::forms::SupportRef::Face { body, .. } = &mut definition.profile.as_mut().unwrap().source else { panic!("face") };
        *body = "Missing support".into();
        h.app.session.refresh();
        let document = serde_json::to_value(h.app.session.doc.as_ref()).unwrap();
        // Programmatic strict Edit refuses and never opens a prompt.
        assert!(h.app.run("form.edit", json!({"feature":feature})).is_err());
        assert!(h.app.forms_prototype.repair_offer.is_none());
        enter(&mut h.app, feature);
        assert!(h.app.session.forms_prototype.is_none());
        assert!(h.app.forms_prototype.repair_offer.is_some());
        h.frames(3);
        assert!(h.ctx.memory(|memory| memory.top_modal_layer().is_some()), "retained entry uses the standard input-blocking modal");
        crate::test_support::click(&mut h, "dialog:ok");
        if let Some(pending) = h.app.forms_prototype.repair_entry.take() {
            let result = pending.done.recv().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(result).unwrap();
            h.app.forms_prototype.repair_entry = Some(RepairEntry { feature: pending.feature, done: rx });
            h.frames(3);
        }
        assert!(h.app.forms_prototype.repair_offer.is_none());
        let retained = h.app.session.forms_prototype.as_ref().unwrap().definition();
        assert!(h.app.session.forms_prototype.as_ref().unwrap().repair.is_some());
        crate::test_support::click(&mut h, "forms_finish");
        assert!(h.app.session.forms_prototype.as_ref().unwrap().repair.is_some());
        assert_eq!(serde_json::to_value(h.app.session.doc.as_ref()).unwrap(), document);
        crate::test_support::click(&mut h, "forms_repair_reattach");
        h.frames(3);
        assert_eq!(tool_panel::inspect(&h.app)["desired"], serde_json::Value::Null, "an explicit mapping is required before preview");
        crate::test_support::click(&mut h, "forms_tool_cancel");
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().definition(), retained);
        assert!(h.app.session.forms_prototype.as_ref().unwrap().repair.is_some());
        crate::test_support::click(&mut h, "forms_cancel");
        assert!(h.app.session.forms_prototype.is_none());
        assert_eq!(serde_json::to_value(h.app.session.doc.as_ref()).unwrap(), document);
    }
    #[test]
    fn retained_entry_worker_discards_cancelled_or_changed_document_and_draft_results() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":4,"width":6,"height":1,"body_name":"Support","operation":"new"})).unwrap();
        let body = app.session.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
        let feature = app.session.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let solidcraft_engine::doc::FeatureKind::Form { definition, .. } = &mut app.session.doc_mut().feature_mut(feature).unwrap().kind else {
            panic!("Form")
        };
        let solidcraft_engine::doc::forms::SupportRef::Face { body, .. } = &mut definition.profile.as_mut().unwrap().source else { panic!("face") };
        *body = "Missing support".into();
        app.session.refresh();
        let commands = [("form.repair".into(), json!({"feature":feature}))];
        let inject = |app: &mut SolveApp, result| {
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(result).unwrap();
            app.forms_prototype.repair_offer = Some((feature, "missing support".into()));
            app.forms_prototype.repair_entry = Some(RepairEntry { feature, done: rx });
        };
        let before = serde_json::to_value(app.session.doc.as_ref()).unwrap();
        let cancelled = app.session.scratch().compute_commands(&commands, None).unwrap();
        inject(&mut app, Ok(cancelled));
        app.forms_prototype.repair_offer = None;
        repair_entry_tick(&mut app);
        assert!(app.session.forms_prototype.is_none());
        assert!(app.forms_prototype.repair_entry.is_none());
        assert_eq!(serde_json::to_value(app.session.doc.as_ref()).unwrap(), before);
        let stale = app.session.scratch().compute_commands(&commands, None).unwrap();
        inject(&mut app, Ok(stale));
        app.session.doc_mut().feature_mut(feature).unwrap().name.push_str(" changed");
        app.session.refresh();
        let changed = serde_json::to_value(app.session.doc.as_ref()).unwrap();
        repair_entry_tick(&mut app);
        assert!(app.session.forms_prototype.is_none());
        assert_eq!(serde_json::to_value(app.session.doc.as_ref()).unwrap(), changed);
        let stale_draft = app.session.scratch().compute_commands(&commands, None).unwrap();
        inject(&mut app, Ok(stale_draft));
        app.session.execute("form.prototype.create", &json!({"primitive":"face","size":8,"segments":[2,2,1]})).unwrap();
        let draft = app.session.forms_prototype.as_ref().unwrap().definition();
        repair_entry_tick(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), draft);
        assert_eq!(serde_json::to_value(app.session.doc.as_ref()).unwrap(), changed);
        app.session.execute("form.prototype.close", &json!({})).unwrap();
        let failed = app.session.scratch().compute_commands(&[("form.repair".into(), json!({"feature":u64::MAX}))], None).map_err(|e| e.to_string());
        let failure = failed.as_ref().err().unwrap().clone();
        inject(&mut app, failed);
        repair_entry_tick(&mut app);
        assert_eq!(app.forms_prototype.repair_offer.as_ref().unwrap().1, failure, "the modal must show the actual failed opening reason");
        assert!(app.session.forms_prototype.is_none());
        assert_eq!(serde_json::to_value(app.session.doc.as_ref()).unwrap(), changed);
        let current = app.session.scratch().compute_commands(&commands, None).unwrap();
        inject(&mut app, Ok(current));
        repair_entry_tick(&mut app);
        assert!(app.session.forms_prototype.as_ref().unwrap().repair.is_some());
        assert!(app.forms_prototype.repair_offer.is_none());
        assert!(app.forms_prototype.repair_entry.is_none());
        assert_eq!(serde_json::to_value(app.session.doc.as_ref()).unwrap(), changed);
    }
    #[test]
    fn returning_to_a_form_tab_rearms_consumed_gpu_model_and_overlay_uploads() {
        for other_form in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.home.open = false;
            app.session.execute("form.prototype.create", &json!({"size":4,"segments":[2,2,2],"levels":1})).unwrap();
            let p = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::new(0.2, 0.0, 0.0);
            app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z]]})).unwrap();
            app.forms_prototype.elements.insert(Element::Face(0));
            selection::sync(&mut app);
            app.forms_prototype.pivot = Some(Vec3::new(2.0, 3.0, 4.0));
            app.forms_prototype.scale = true;
            app.viewport.gpu = Some(crate::gpu::GpuTarget {
                format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
                depth: Some(egui_wgpu::wgpu::TextureFormat::Depth32Float),
                samples: 1,
                tess: false,
                oit: false,
            });
            let ctx = egui::Context::default();
            let draw = |app: &mut SolveApp| {
                let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
                let projection = crate::viewport::projection(app, rect);
                let _ = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                    render(app, ui, &projection);
                });
                viewport_frame(app, &projection, 1.0, app.forms_prototype.uploaded)
            };
            let original = app.session.forms_prototype.as_ref().unwrap().inspect();
            let selection = app.forms_prototype.selected.clone();
            let first = draw(&mut app);
            // Match Renderer::prepare/Batches::take: the shared GPU renderer owns
            // the bytes after consuming all four CPU upload slots.
            let body = first.slot.lock().unwrap().take().unwrap().tris;
            let highlights = first.hl_slot.lock().unwrap().take().unwrap().tris;
            assert!(!body.is_empty() && !highlights.is_empty());
            for slot in [&first.sketch_slot, &first.pv_slot] {
                let empty = slot.lock().unwrap().take().unwrap();
                assert!(empty.tris.is_empty() && empty.lines.is_empty());
            }
            crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
            assert_eq!(app.docs.active, 1);
            if other_form {
                app.session.execute("form.prototype.create", &json!({"size":7,"segments":[1,1,1],"levels":1})).unwrap();
                let other_source = app.session.forms_prototype.as_ref().unwrap().inspect();
                let other = draw(&mut app);
                assert_ne!(first.key, other.key);
                for slot in [&other.slot, &other.sketch_slot, &other.hl_slot, &other.pv_slot] {
                    assert!(slot.lock().unwrap().take().is_some());
                }
                crate::documents::switch(&mut app, 0);
                let returned = draw(&mut app);
                assert_eq!(returned.slot.lock().unwrap().as_ref().unwrap().tris, body, "A's model must be available to replace B's GPU batch");
                assert_eq!(returned.hl_slot.lock().unwrap().as_ref().unwrap().tris, highlights);
                crate::documents::switch(&mut app, 1);
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), other_source);
                crate::documents::switch(&mut app, 0);
            } else {
                assert!(app.session.forms_prototype.is_none());
                crate::documents::switch(&mut app, 0);
            }
            assert_eq!(app.docs.active, 0);
            let returned = draw(&mut app);
            assert!(Arc::ptr_eq(&first.slot, &returned.slot), "restore retains this tab's slot identity");
            assert_eq!(returned.slot.lock().unwrap().as_ref().unwrap().tris, body);
            assert_eq!(returned.hl_slot.lock().unwrap().as_ref().unwrap().tris, highlights);
            for slot in [&returned.sketch_slot, &returned.pv_slot] {
                let empty = slot.lock().unwrap().as_ref().unwrap().clone();
                assert!(
                    empty.tris.is_empty() && empty.lines.is_empty() && empty.overlay.is_empty() && empty.xray.is_empty(),
                    "the other tab's origin planes/previews must be replaced by explicit empty uploads"
                );
            }
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), original);
            assert_eq!(app.forms_prototype.elements, BTreeSet::from([Element::Face(0)]));
            assert_eq!(app.forms_prototype.selected, selection);
            assert_eq!(app.forms_prototype.pivot, Some(Vec3::new(2.0, 3.0, 4.0)));
            assert!(app.forms_prototype.scale);
        }
    }
    #[test]
    fn zoomed_viewport_points_tools_and_gizmo_stay_clipped_while_panel_and_visible_picks_work() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"primitive":"plane","size":4,"segments":[2,2,2],"levels":2})).unwrap();
        h.frames(3);
        h.app.cam.yaw = 0.0;
        h.app.cam.pitch = std::f64::consts::FRAC_PI_2;
        h.app.cam.fov = 0.0;
        h.app.cam.distance = 8.0;
        h.app.cam.target = Vec3::ZERO;
        live_cad::install_for_test(&mut h.app);
        let descriptors = solidcraft_engine::forms_prototype::insertion_curves(&h.app.session, controls::tolerance(&h.app).unwrap(), &[]).unwrap();
        curves::install_for_test(&mut h.app, Ok(descriptors));
        h.frames(3);
        let source = h.app.session.forms_prototype.as_ref().unwrap().inspect();
        crate::test_support::click(&mut h, "forms_insert_points");
        let rect = h.app.viewport.rect.unwrap();
        let projection = crate::viewport::projection(&h.app, rect);
        let (point, position) = h
            .app
            .session
            .forms_prototype
            .as_ref()
            .unwrap()
            .surface
            .boundaries
            .iter()
            .map(|(edge, _)| {
                let point = curves::get(&h.app, *edge).unwrap().at(0.37).unwrap();
                (point, projection.to_screen(point).unwrap())
            })
            .max_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
            .unwrap();
        assert!(rect.shrink(10.0).contains(position));
        let (_, _, marker) = curves::nearest(&h.app, &projection, position).unwrap();
        assert!(point.dist(marker) < 0.01);
        h.frame_with(crate::test_support::pointer(position, true));
        h.frame_with(crate::test_support::pointer(position, false));
        h.frames(3);
        assert_eq!(tools::point_count_for_test(&h.app), 1);
        // Pan the authored surface rather than fabricating an out-of-bounds paint shape.
        // The placed Insert Point marker and source points now project over the panel.
        let (right, _, _) = h.app.cam.basis();
        let pixels = projection.to_screen(marker + right).unwrap().x - projection.to_screen(marker).unwrap().x;
        h.app.cam.target += right * f64::from((position.x - (rect.right() + 35.0)) / pixels);
        let panel_pick = crate::scenario::handle_at("forms_pick_vertex").unwrap();
        assert!(panel_pick.x > rect.right());
        h.frame_with(vec![egui::Event::PointerMoved(panel_pick)]);
        h.frames(2);
        let projection = crate::viewport::projection(&h.app, rect);
        let marker_screen = projection.to_screen(marker).unwrap();
        assert!(marker_screen.x > rect.right() + 20.0);
        let mut placed_marker = false;
        let mut outside_point = false;
        let mut crossing_mesh = false;
        let mut panel_text = false;
        for clipped in &h.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                let viewport_shape = match shape {
                    egui::Shape::Circle(circle) => {
                        let placed = circle.center.distance(marker_screen) < 0.1 && circle.radius == 5.0 && circle.fill == Tokens::get().accent;
                        placed_marker |= placed;
                        let outside = h.app.forms_prototype.projected.iter().any(|(_, p)| p.x > rect.right() && circle.center.distance(*p) < 0.1)
                            && circle.radius >= 3.0;
                        outside_point |= outside;
                        placed || outside
                    }
                    egui::Shape::Mesh(mesh) => {
                        let crossing = mesh.vertices.len() == 3
                            && mesh.vertices.iter().all(|v| v.color == Tokens::get().body)
                            && mesh.vertices.iter().any(|v| v.pos.x > rect.right())
                            && mesh.vertices.iter().any(|v| v.pos.x < rect.right());
                        crossing_mesh |= crossing;
                        crossing
                    }
                    egui::Shape::Text(text) => {
                        panel_text |= text.pos.x > rect.right() && clipped.clip_rect.contains(panel_pick);
                        false
                    }
                    _ => false,
                };
                if viewport_shape {
                    assert!(rect.contains_rect(clipped.clip_rect), "viewport paint leaked into the panel: {:?}", clipped.clip_rect);
                }
            });
        }
        assert!(placed_marker, "the placed marker must remain in the paint list while the pointer is over the panel");
        assert!(outside_point && crossing_mesh && panel_text);
        crate::test_support::click(&mut h, "forms_insert_points");
        crate::test_support::click(&mut h, "forms_pick_vertex");
        assert!(matches!(h.app.forms_prototype.pick, Pick::Vertex));
        let (index, visible) = h.app.forms_prototype.projected.iter().copied().find(|(_, p)| rect.shrink(30.0).contains(*p)).unwrap();
        h.frame_with(crate::test_support::pointer(visible, true));
        h.frame_with(crate::test_support::pointer(visible, false));
        h.frames(2);
        assert_eq!(h.app.forms_prototype.elements, BTreeSet::from([Element::Vertex(index)]));
        let border = egui::pos2(rect.right() - 3.0, rect.center().y);
        let (origin, direction) = projection.ray(border);
        let pivot = origin + direction * (-origin.z / direction.z);
        h.app.forms_prototype.pivot = Some(pivot);
        h.frame_with(vec![egui::Event::PointerMoved(panel_pick)]);
        let center = projection.to_screen(pivot).unwrap();
        let mut ball = false;
        for clipped in &h.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Circle(circle) = shape
                    && circle.center.distance(center) < 0.1
                    && circle.radius == 6.0
                {
                    ball = true;
                    assert!(rect.contains_rect(clipped.clip_rect));
                }
            });
        }
        assert!(ball, "the transform handle must still be painted at the custom pivot");
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), source);
    }
    #[test]
    fn fresh_and_selected_edges_double_click_expand_the_same_loop_at_sixty_hertz() {
        for preselected in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("form.prototype.create", json!({"primitive":"plane","size":4,"segments":[2,2,2],"levels":2})).unwrap();
            h.app.preferences.forms_greedy_selection = false;
            h.app.forms_prototype.pick = Pick::Edge;
            h.app.cam.target = Vec3::ZERO;
            h.app.cam.distance = 12.0;
            h.app.cam.yaw = 0.65;
            h.app.cam.pitch = 0.9;
            h.app.cam.fov = 0.0;
            live_cad::install_for_test(&mut h.app);
            let descriptors =
                solidcraft_engine::forms_prototype::insertion_curves(&h.app.session, controls::tolerance(&h.app).unwrap(), &[]).unwrap();
            curves::install_for_test(&mut h.app, Ok(descriptors));
            h.frames(3);
            let source = h.app.session.forms_prototype.as_ref().unwrap().inspect();
            let form = h.app.session.forms_prototype.as_ref().unwrap();
            let edge = form
                .surface
                .boundaries
                .iter()
                .map(|(edge, _)| *edge)
                .find(|edge| edge.iter().all(|&id| form.cage.points[id].x.abs() < 1e-10))
                .unwrap();
            let projection = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
            let position = projection.to_screen(curves::get(&h.app, edge).unwrap().at(0.5).unwrap()).unwrap();
            assert_eq!(surface::nearest(&h.app, &projection, position), Some(Element::Edge(edge)));
            assert!(h.app.forms_prototype.elements.is_empty());
            let pointer = |pressed| egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            h.frame_with(vec![egui::Event::PointerMoved(position)]);
            if preselected {
                h.frame_with(vec![pointer(true)]);
                h.frame_with(vec![pointer(false)]);
                h.frames(60);
                assert_eq!(h.app.forms_prototype.elements, [Element::Edge(edge)].into_iter().collect());
            }
            // The Harness advances precisely 1/60 s per frame. Both releases occur 1/30 s apart,
            // independent of worker scheduling, native redraw load or control round trips.
            h.frame_with(vec![pointer(true)]);
            h.frame_with(vec![pointer(false)]);
            h.frame_with(vec![pointer(true)]);
            h.frame_with(vec![pointer(false)]);
            assert!(h.ctx.input(|input| input.pointer.button_double_clicked(egui::PointerButton::Primary)));
            h.frames(3);
            let form = h.app.session.forms_prototype.as_ref().unwrap();
            let wanted: BTreeSet<_> = form
                .surface
                .boundaries
                .iter()
                .map(|(edge, _)| *edge)
                .filter(|edge| edge.iter().all(|&id| form.cage.points[id].x.abs() < 1e-10))
                .map(Element::Edge)
                .collect();
            assert_eq!(wanted.len(), 2);
            assert_eq!(h.app.forms_prototype.elements, wanted, "preselected={preselected}");
            assert_eq!(h.app.forms_prototype.selected.len(), 3);
            assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), source);
        }
    }
    fn source_curve_pick_fixture() -> (SolveApp, Vec<solidcraft_engine::doc::forms::EdgeCurve>, [usize; 2], Proj, Pos2) {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":0})).unwrap();
        let p = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::new(2.0, 0.0, 3.0);
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z]]})).unwrap();
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        let source = solidcraft_engine::forms_prototype::insertion_curves(&app.session, 0.01, &[]).unwrap();
        let edge = [0, 1];
        let curve = source.iter().find(|curve| curve.edge == edge).unwrap();
        let mut camera = app.cam;
        camera.yaw = 0.0;
        camera.pitch = std::f64::consts::FRAC_PI_2;
        camera.fov = 0.0;
        camera.target = Vec3::ZERO;
        camera.distance = 700.0 / (2.0 * 60.0 * 0.4);
        let projection =
            Proj { cam: camera, vp: camera.view_proj(900.0 / 700.0, 20.0), rect: egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)) };
        let position = projection.to_screen(curve.at(0.3).unwrap()).unwrap();
        (app, source, edge, projection, position)
    }

    #[test]
    fn fitted_cad_edge_picking_uses_source_curves_at_level_zero_before_face_selection() {
        let (mut app, source, edge, projection, position) = source_curve_pick_fixture();
        let original = app.session.forms_prototype.as_ref().unwrap().inspect();
        assert!(app.forms_prototype.elements.is_empty() && app.forms_prototype.selected.is_empty());
        // The real source curve is over25px away from its level0 chord here.
        assert_eq!(surface::nearest_with(&app, &projection, position, Pick::Edge), None);
        curves::install_for_test(&mut app, Ok(source.clone()));
        assert_eq!(surface::nearest_with(&app, &projection, position, Pick::Edge), None, "pre-CAD fallback is unchanged");
        live_cad::install_for_test(&mut app);
        let pending = curves::pending_for_test(&mut app);
        assert_eq!(curves::inspect(&app)["pending"], true);
        assert_eq!(curves::inspect(&app)["ready"], false);
        assert_eq!(surface::nearest_with(&app, &projection, position, Pick::Edge), None, "CAD-ready picking waits for the exact source cache");
        pending.send(Ok(source)).unwrap();
        curves::poll_for_test(&mut app);
        assert_eq!(curves::inspect(&app)["pending"], false);
        assert_eq!(curves::inspect(&app)["ready"], true);
        assert_eq!(surface::nearest_with(&app, &projection, position, Pick::Edge), Some(Element::Edge(edge)));
        assert_eq!(surface::nearest_with(&app, &projection, position, Pick::All), Some(Element::Edge(edge)));
        assert_eq!(surface::nearest_with(&app, &projection, position, Pick::Vertex), None);
        assert!(!matches!(surface::nearest_with(&app, &projection, position, Pick::Face), Some(Element::Edge(_))));
        let curve = curves::get(&app, edge).unwrap();
        let a = projection.to_screen(curve.at(0.29).unwrap()).unwrap();
        let b = projection.to_screen(curve.at(0.31).unwrap()).unwrap();
        let tangent = (b - a).normalized();
        let outside = position + egui::vec2(-tangent.y, tangent.x) * 12.0;
        assert!(curves::project(curve, &projection, outside).is_none(), "the exact hit threshold remains9px");
        let vertex = projection.to_screen(app.session.forms_prototype.as_ref().unwrap().surface.vertices[0]).unwrap();
        assert_eq!(surface::nearest_with(&app, &projection, vertex, Pick::All), Some(Element::Vertex(0)));
        assert!(matches!(surface::nearest_with(&app, &projection, vertex, Pick::Edge), Some(Element::Edge(_))));
        let highlight = surface::highlight_curve(&app, edge).unwrap();
        assert_eq!(highlight.len(), 65);
        for (i, point) in highlight.iter().enumerate() {
            assert!(point.dist(curve.at(i as f64 / 64.0).unwrap()) < 1e-12);
        }
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), original);
        assert!(app.session.doc.features.is_empty());
    }

    #[test]
    fn source_curve_cache_refuses_changed_and_identical_replacement_drafts_and_other_tabs() {
        let (mut app, source, _, projection, position) = source_curve_pick_fixture();
        live_cad::install_for_test(&mut app);
        curves::install_for_test(&mut app, Ok(source.clone()));
        assert!(curves::nearest(&app, &projection, position).is_some());
        let p = app.session.forms_prototype.as_ref().unwrap().cage.points[0];
        let sender = curves::pending_for_test(&mut app);
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x+0.1,p.y,p.z]]})).unwrap();
        sender.send(Ok(source.clone())).unwrap();
        curves::poll_for_test(&mut app);
        assert!(curves::ready(&app).is_none());
        assert!(curves::nearest(&app, &projection, position).is_none());

        let (mut app, source, _, projection, position) = source_curve_pick_fixture();
        live_cad::install_for_test(&mut app);
        let original = app.session.forms_prototype.as_ref().unwrap().inspect();
        let initial = curves::source(&app).unwrap();
        curves::install_for_test(&mut app, Ok(source));
        app.session.execute("form.prototype.close", &json!({})).unwrap();
        app.session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":0})).unwrap();
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-3,-5,8]]})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), original);
        assert_ne!(curves::source(&app).unwrap(), initial);
        assert!(curves::ready(&app).is_none());
        assert!(curves::nearest(&app, &projection, position).is_none());

        let (mut app, source, _, projection, position) = source_curve_pick_fixture();
        live_cad::install_for_test(&mut app);
        let document = app.session.document_id.clone();
        curves::install_for_test(&mut app, Ok(source));
        crate::documents::new_design(&mut app);
        assert_ne!(app.session.document_id, document);
        assert!(curves::ready(&app).is_none());
        assert!(curves::nearest(&app, &projection, position).is_none());
        crate::documents::switch(&mut app, 0);
        assert_eq!(app.session.document_id, document);
        assert!(curves::nearest(&app, &projection, position).is_some(), "an unchanged tab keeps its own current source cache");
        curves::clear_for_test(&mut app);
        assert!(!curves::current(&app));
    }
    #[test]
    fn preferences_draw_the_forms_settings_while_an_edit_session_stays_open() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"segments":[1,1,1]})).unwrap();
        let original = h.app.session.forms_prototype.as_ref().unwrap().definition();
        h.frames(3);
        h.call("ui.prefs", json!({"open":"Design"}));
        h.frames(4);
        assert!(h.app.prefs_window.open);
        assert!(h.at(&json!({"handle":"prefs:forms_mesh_while_dragging"})).is_ok());
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().definition(), original);
        h.call("ui.prefs", json!({"button":"cancel"}));
        h.frames(2);
        assert!(!h.app.prefs_window.open);
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().definition(), original);
    }
    #[test]
    fn shift_ranges_ctrl_toggling_and_live_greedy_preferences_use_logical_points() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"primitive":"face","segments":[3,3,1]})).unwrap();
        h.app.forms_prototype.pick = Pick::Vertex;
        h.frames(3);
        let position = |h: &crate::scenario::Harness, id: usize| {
            let f = h.app.session.forms_prototype.as_ref().unwrap();
            let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
            proj.to_screen(f.surface.vertices[id]).unwrap()
        };
        let a = position(&h, 0);
        let b = position(&h, 15);
        h.call("ui.click", json!({"x":a.x,"y":a.y}));
        crate::test_support::play(&mut h);
        h.call("ui.click", json!({"x":b.x,"y":b.y,"shift":true}));
        crate::test_support::play(&mut h);
        assert_eq!(h.app.forms_prototype.elements.len(), 16);
        let p = position(&h, 5);
        h.call("ui.click", json!({"x":p.x,"y":p.y,"ctrl":true}));
        crate::test_support::play(&mut h);
        assert_eq!(h.app.forms_prototype.elements.len(), 15);
        h.app.forms_prototype.elements.clear();
        h.app.forms_prototype.selected.clear();
        h.call("ui.prefs", json!({"forms_greedy_selection":true}));
        for p in [a, b] {
            h.call("ui.click", json!({"x":p.x,"y":p.y}));
            crate::test_support::play(&mut h);
        }
        assert_eq!(h.app.forms_prototype.elements.len(), 2);
        h.call("ui.prefs", json!({"forms_greedy_selection":false}));
        let p = position(&h, 4);
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        crate::test_support::play(&mut h);
        assert_eq!(h.app.forms_prototype.elements.len(), 1);
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
    }
    use egui::Rect;
    #[test]
    fn model_upload_is_never_shared_with_cached_overlay_batches() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({})).unwrap();
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let frame = viewport_frame(&app, &proj, 1.0, 123);
        assert_ne!(frame.sketch_key, 0);
        assert!(!Arc::ptr_eq(&frame.slot, &frame.sketch_slot));
        assert!(!Arc::ptr_eq(&frame.sketch_slot, &frame.hl_slot));
        assert!(!Arc::ptr_eq(&frame.hl_slot, &frame.pv_slot));
        for slot in [&frame.sketch_slot, &frame.hl_slot, &frame.pv_slot] {
            let scene = slot.lock().unwrap().take().unwrap();
            assert!(scene.tris.is_empty() && scene.lines.is_empty() && scene.overlay.is_empty() && scene.xray.is_empty());
        }
        // Build real upload scenes and callbacks without executing a GPU callback.
        // The model and face highlight must stay separate even on a headless host.
        app.viewport.gpu = Some(crate::gpu::GpuTarget {
            format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
            depth: Some(egui_wgpu::wgpu::TextureFormat::Depth32Float),
            samples: 1,
            tess: false,
            oit: false,
        });
        let ctx = egui::Context::default();
        let draw = |app: &mut SolveApp| {
            let _ =
                crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(proj.rect), ..Default::default() }, |ui| render(app, ui, &proj));
            viewport_frame(app, &proj, 1.0, app.forms_prototype.uploaded)
        };
        app.forms_prototype.elements.insert(Element::Face(0));
        app.forms_prototype.hover = Some(Element::Face(1));
        let highlighted = draw(&mut app);
        let body = highlighted.slot.lock().unwrap().as_ref().unwrap().tris.clone();
        let tint = highlighted.hl_slot.lock().unwrap().as_ref().unwrap().tris.clone();
        let expected = app.session.forms_prototype.as_ref().unwrap().surface.mesh.tri_face.iter().filter(|&&face| face == 0 || face == 1).count()
            * 3
            * crate::gpu::TRI_SIZE;
        assert!(!body.is_empty() && !tint.is_empty());
        assert_eq!(tint.len(), expected, "only the selected and hovered logical faces enter the depth-tested highlight pass");
        assert!(!Arc::ptr_eq(&highlighted.slot, &highlighted.hl_slot));
        app.forms_prototype.elements.clear();
        app.forms_prototype.hover = Some(Element::Face(2));
        let moved = draw(&mut app);
        assert_ne!(moved.hl_key, highlighted.hl_key, "a hover change invalidates the cached highlight batch");
        assert_eq!(moved.slot.lock().unwrap().as_ref().unwrap().tris, body, "hover does not recolor or move the ordinary model upload");
        assert_ne!(moved.hl_slot.lock().unwrap().as_ref().unwrap().tris, tint);
        app.forms_prototype.hover = None;
        let cleared = draw(&mut app);
        assert_ne!(cleared.hl_key, moved.hl_key);
        assert!(
            cleared.hl_slot.lock().unwrap().as_ref().unwrap().tris.is_empty(),
            "clear must upload an empty scene rather than retain a previous tint"
        );
        assert_eq!(cleared.slot.lock().unwrap().as_ref().unwrap().tris, body);
    }
    #[test]
    fn all_filter_paints_unselected_surface_points_and_clicks_them() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({})).unwrap();
        h.frames(3);
        assert!(h.app.forms_prototype.selected.is_empty());
        let positions = h.app.forms_prototype.projected.clone();
        assert!(positions.len() >= 4);
        for &(_, p) in &positions {
            let mut painted = false;
            for clipped in &h.shapes {
                crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Circle(c) = shape
                        && c.center.distance(p) < 0.1
                        && c.radius >= 3.0
                    {
                        painted = true;
                    }
                });
            }
            assert!(painted, "surface point at {p:?} was hidden with the default All filter");
        }
        let (index, p) = positions[0];
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        crate::test_support::play(&mut h);
        assert_eq!(h.app.forms_prototype.elements, BTreeSet::from([Element::Vertex(index)]));
    }
    #[test]
    fn shift_gizmo_extrudes_box_and_quadball_even_when_scale_is_selected() {
        for primitive in ["box", "quadball"] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("form.prototype.create", json!({"primitive":primitive})).unwrap();
            h.frames(3);
            let f = h.app.session.forms_prototype.as_ref().unwrap();
            let before = f.cage.clone();
            let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
            let pos = f
                .logical
                .faces
                .iter()
                .enumerate()
                .find_map(|(id, face)| {
                    let p = face.iter().map(|&i| f.surface.vertices[i]).fold(Vec3::ZERO, |s, p| s + p) / face.len() as f64;
                    let screen = proj.to_screen(p)?;
                    (surface::nearest(&h.app, &proj, screen) == Some(Element::Face(id))).then_some(screen)
                })
                .unwrap();
            h.call("ui.click", json!({"x":pos.x,"y":pos.y}));
            crate::test_support::play(&mut h);
            crate::test_support::click(&mut h, "forms_scale_mode");
            let [x0, y0] = h.at(&json!({"gizmo":"x"})).unwrap();
            let [x1, y1] = h.at(&json!({"gizmo":"x","by":[8,0,0]})).unwrap();
            h.call("ui.drag", json!({"x0":x0,"y0":y0,"x1":x1,"y1":y1,"shift":true,"steps":8}));
            crate::test_support::play(&mut h);
            let after = h.app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(after.cage.faces.len(), before.faces.len() + 4, "{primitive}: {:?}", h.app.status);
            assert_eq!(after.inspect()["undo"], 1);
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().cage, before);
            crate::test_support::click(&mut h, "forms_pick_all");
            let end = pos + egui::vec2(35.0, -20.0);
            let shift = egui::Modifiers { shift: true, ..Default::default() };
            h.app.synthetic.extend([
                egui::Event::ModifiersChanged(egui::Modifiers::NONE),
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
                egui::Event::ModifiersChanged(shift),
                egui::Event::PointerMoved(end),
                egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: shift },
            ]);
            crate::test_support::play(&mut h);
            let after = h.app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(after.cage.faces.len(), before.faces.len() + 4, "Shift pressed after mouse-down: {primitive}");
            assert_eq!(after.inspect()["undo"], 1);
        }
    }
    #[test]
    fn shift_drag_extrudes_once_and_undo_restores_source() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1]})).unwrap();
        fit(&mut app);
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        app.forms_prototype.pick = Pick::Face;
        let form = app.session.forms_prototype.as_ref().unwrap();
        let cage = form.cage.clone();
        let start = form
            .logical
            .faces
            .iter()
            .enumerate()
            .find_map(|(id, face)| {
                let center = face.iter().map(|&i| form.surface.vertices[i]).fold(Vec3::ZERO, |s, p| s + p) / face.len() as f64;
                let p = proj.to_screen(center)?;
                (surface::nearest(&app, &proj, p) == Some(Element::Face(id))).then_some(p)
            })
            .unwrap();
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        begin_drag(&mut app, &proj, start, egui::Modifiers { shift: true, ..Default::default() });
        for distance in [35.0, 70.0] {
            pull(&mut app, start + egui::vec2(distance, -20.0));
            let f = app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(f.cage.faces.len(), 10);
            assert_eq!(f.inspect()["undo"], 1);
        }
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage, cage);
    }

    #[test]
    fn shift_drag_of_refined_free_edge_selects_the_translated_wall_rim() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"primitive":"face","dimensions":[10,10,1],"segments":[2,2,1],"levels":1})).unwrap();
        h.app.run("form.prototype.refine", json!({"faces":[0],"counts":[2,2]})).unwrap();
        h.app.forms_prototype.pick = Pick::Edge;
        fit(&mut h.app);
        h.frames(3);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let ring = form.cage.boundary_loops().unwrap().remove(0);
        let rim: BTreeSet<_> =
            ring.iter().copied().zip(ring.iter().copied().cycle().skip(1)).take(ring.len()).map(|(a, b)| (a.min(b), a.max(b))).collect();
        let layout = form.hierarchy.as_ref().unwrap().layout().unwrap();
        let (start, wall_count) = layout
            .paths
            .iter()
            .filter(|(_, path)| path.len() > 2 && path.windows(2).all(|p| rim.contains(&(p[0].min(p[1]), p[0].max(p[1])))))
            .find_map(|(edge, path)| {
                let (_, points) = form.surface.boundaries.iter().find(|(e, _)| e == edge)?;
                let point = proj.to_screen(*points.get(points.len() / 2)?)?;
                (surface::nearest(&h.app, &proj, point) == Some(Element::Edge(*edge))).then_some((point, path.len() - 1))
            })
            .unwrap();
        h.call("ui.drag", json!({"x0":start.x,"y0":start.y,"x1":start.x+35.0,"y1":start.y-20.0,"shift":true,"steps":8}));
        crate::test_support::play(&mut h);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.cage.faces.len(), before.cage.faces.len() + wall_count, "{:?}", h.app.status);
        assert_eq!(form.inspect()["undo"], 2);
        let front: BTreeSet<_> = form.extruded_edges().iter().copied().map(Element::Edge).collect();
        assert_eq!(front.len(), wall_count);
        assert_eq!(h.app.forms_prototype.elements, front);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn shift_drag_on_a_refined_mirror_face_extrudes_both_sides_once() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[16,12,1],"segments":[4,3,1],"levels":1})).unwrap();
        app.session.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let root = {
            let f = app.session.forms_prototype.as_ref().unwrap();
            f.logical.faces.iter().position(|face| face.iter().all(|&v| f.cage.points[v].x >= 4.0)).unwrap()
        };
        app.session.execute("form.prototype.refine", &json!({"faces":[root],"counts":[2,2]})).unwrap();
        fit(&mut app);
        app.forms_prototype.pick = Pick::Face;
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let f = app.session.forms_prototype.as_ref().unwrap();
        let before = f.definition();
        let checkpoint_count = f.inspect()["undo"].as_u64().unwrap();
        let start = f
            .logical
            .faces
            .iter()
            .enumerate()
            .skip(before.hierarchy.as_ref().unwrap().base.faces.len())
            .find_map(|(id, face)| {
                if face.is_empty() || !face.iter().all(|&v| f.cage.points[v].x > 3.5) {
                    return None;
                }
                let center = face.iter().map(|&i| f.surface.vertices[i]).fold(Vec3::ZERO, |a, b| a + b) / face.len() as f64;
                let p = proj.to_screen(center)?;
                (surface::nearest(&app, &proj, p) == Some(Element::Face(id))).then_some(p)
            })
            .unwrap();
        begin_drag(&mut app, &proj, start, egui::Modifiers { shift: true, ..Default::default() });
        for distance in [22.0, 44.0] {
            pull(&mut app, start + egui::vec2(distance, -15.0));
            let f = app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(f.cage.faces.len(), before.cage.faces.len() + 8, "{:?}", app.status);
            assert_eq!(f.inspect()["undo"], checkpoint_count + 1);
            f.symmetry.as_ref().unwrap().validate_reflection(&f.cage).unwrap();
        }
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn shift_drag_of_a_trimmed_edge_selects_editable_wall_rim() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":1})).unwrap();
        h.app.run("form.prototype.refine", json!({"faces":[5],"counts":[2,2]})).unwrap();
        let old = h.app.session.forms_prototype.as_ref().unwrap().logical.faces[16].clone();
        h.app.run("form.prototype.topology", json!({"tool":"delete_faces","faces":[16]})).unwrap();
        h.app.forms_prototype.pick = Pick::Edge;
        fit(&mut h.app);
        h.frames(3);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let hierarchy = form.hierarchy.as_ref().unwrap();
        let start = old
            .iter()
            .copied()
            .zip(old.iter().copied().cycle().skip(1))
            .take(old.len())
            .find_map(|(a, b)| {
                let attachment = hierarchy.trim_attachments(&[[a, b]]).ok()?.into_iter().find(|t| t.root == 5)?;
                let key = [attachment.edge[0].min(attachment.edge[1]), attachment.edge[0].max(attachment.edge[1])];
                let (_, path) = form.surface.boundaries.iter().find(|(edge, _)| *edge == key)?;
                let point = proj.to_screen(path[path.len() / 2])?;
                (surface::nearest(&h.app, &proj, point) == Some(Element::Edge(key))).then_some(point)
            })
            .unwrap();
        h.call("ui.drag", json!({"x0":start.x,"y0":start.y,"x1":start.x+23.0,"y1":start.y-32.0,"shift":true,"steps":8}));
        crate::test_support::play(&mut h);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.cage, before.cage, "{:?}", h.app.status);
        let first = before.cage.points.len();
        assert_eq!(form.extruded_edges(), &[[first, first + 1]], "{:?}", h.app.status);
        assert_eq!(h.app.forms_prototype.elements, BTreeSet::from([Element::Edge([first, first + 1])]));
        assert!(form.selectable_control(first));
        assert_eq!(form.logical.faces.len(), before.hierarchy.unwrap().leaves.len() + 1);
        assert_eq!(form.inspect()["undo"], 3);
        let wall = form.logical.faces.last().unwrap();
        let p = |i| form.surface.vertices[wall[i]];
        let tangent = (p(1) - p(0)).normalized().unwrap();
        let sweep = (p(2) - p(1)).normalized().unwrap();
        h.app.forms_prototype.orientation = 2;
        h.app.forms_prototype.gizmo_drag = None;
        let directions = basis(&h.app);
        assert!(directions[2].dot(tangent).abs() < 1e-6);
        assert!(directions[2].dot(sweep).abs() < 1e-6);
        assert!(directions[0].cross(directions[1]).dot(directions[2]) > 0.999999);
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(h.app.session.forms_prototype.as_ref().unwrap().hierarchy.as_ref().is_some_and(|h| !h.has_trim_wall()));
    }

    #[test]
    fn screen_drag_moves_picked_vertices_and_undo_restores_them() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({})).unwrap();
        fit(&mut app);
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let cage = app.session.forms_prototype.as_ref().unwrap().cage.clone();
        let start = proj.to_screen(app.session.forms_prototype.as_ref().unwrap().surface.vertices[0]).unwrap();
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        assert!(!app.forms_prototype.selected.is_empty());
        pull(&mut app, start + egui::vec2(70.0, -40.0));
        assert_ne!(app.session.forms_prototype.as_ref().unwrap().cage, cage);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage, cage);
    }

    #[test]
    fn new_form_tolerance_is_a_preference_and_saved_forms_keep_theirs() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        for invalid in [0.0, -0.01, 1.01] {
            assert!(crate::prefs::control(&mut app, &json!({"forms_cad_tolerance":invalid})).is_err());
            assert_eq!(app.preferences.forms_cad_tolerance, 0.05);
        }
        for value in [0.05, 0.01] {
            crate::prefs::control(&mut app, &json!({"forms_cad_tolerance":value})).unwrap();
            app.session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
            opened(&mut app);
            assert_eq!(controls::tolerance(&app), Some(value));
            app.session.execute("form.prototype.close", &json!({})).unwrap();
        }
        app.preferences.forms_cad_tolerance = 0.05;
        app.session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
        let id = app.session.execute("form.prototype.commit", &json!({"tolerance":0.02})).unwrap()["feature"].as_u64().unwrap();
        app.session.execute("form.edit", &json!({"feature":id})).unwrap();
        opened(&mut app);
        assert_eq!(controls::tolerance(&app), Some(0.02));
    }
    #[test]
    fn numeric_fields_accept_units_and_keep_angles_and_scale_distinct() {
        use solidcraft_engine::doc::expr::Kind;
        let app = SolveApp::new(Default::default(), Default::default());
        assert_eq!(controls::number(&app, "1 cm + 2 mm", Kind::Length), Some(12.0));
        assert!((controls::number(&app, "90 deg", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((controls::number(&app, "90", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(controls::number(&app, "3/2", Kind::Unitless), Some(1.5));
        assert_eq!(controls::number(&app, "2 mm", Kind::Unitless), None);
        assert_eq!(controls::number(&app, "NaN", Kind::Length), None);
        assert_eq!(controls::number(&app, "inf", Kind::Length), None);
    }
    #[test]
    fn keyboard_selection_and_deletion_stay_inside_the_form() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":1})).unwrap();
        let primary = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
        app.forms_prototype.pick = Pick::Vertex;
        assert!(shortcut(&mut app, egui::Key::A, primary));
        assert_eq!(app.forms_prototype.elements.len(), 9);
        assert!(app.forms_prototype.elements.iter().all(|e| matches!(e, Element::Vertex(_))));
        assert!(shortcut(&mut app, egui::Key::Delete, egui::Modifiers::NONE));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage.faces.len(), 4);
        app.forms_prototype.pick = Pick::Edge;
        shortcut(&mut app, egui::Key::A, primary);
        assert_eq!(app.forms_prototype.elements.len(), 12);
        app.forms_prototype.elements = [Element::Face(0)].into_iter().collect();
        selection::sync(&mut app);
        assert!(shortcut(&mut app, egui::Key::Delete, egui::Modifiers::NONE));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage.faces.len(), 3);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 1);
        assert!(app.session.doc.features.is_empty());
        assert!(app.forms_prototype.elements.is_empty());
        app.forms_prototype.pick = Pick::Face;
        shortcut(&mut app, egui::Key::A, primary);
        assert_eq!(app.forms_prototype.elements.len(), 3);
        assert!(!shortcut(&mut app, egui::Key::F, egui::Modifiers::NONE));
        tool_panel::start(&mut app, tool_panel::Mode::Thicken);
        shortcut(&mut app, egui::Key::Delete, egui::Modifiers::NONE);
        assert!(app.session.forms_prototype.as_ref().unwrap().has_tool_preview());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage.faces.len(), 3);
        tool_panel::cancel(&mut app);
    }
    #[test]
    fn axis_pull_changes_only_the_requested_coordinate() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({})).unwrap();
        fit(&mut app);
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let cage = app.session.forms_prototype.as_ref().unwrap().cage.clone();
        let start = proj.to_screen(app.session.forms_prototype.as_ref().unwrap().surface.vertices[0]).unwrap();
        app.forms_prototype.axis = Axis::X;
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        let selected = app.forms_prototype.selected.clone();
        pull(&mut app, start + egui::vec2(70.0, -20.0));
        let changed = &app.session.forms_prototype.as_ref().unwrap().cage;
        assert_ne!(changed, &cage);
        for (i, (a, b)) in cage.points.iter().zip(&changed.points).enumerate() {
            if selected.contains(&i) {
                assert_eq!(a.y, b.y);
                assert_eq!(a.z, b.z);
            } else {
                assert_eq!(a, b);
            }
        }
    }
}
