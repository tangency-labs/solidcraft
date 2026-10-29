//! Opt-in local Forms editing experiment, inside SolidCraft's normal viewport shell.
use crate::gpu::{Frame, GpuScene, SceneSlot, ViewportCallback};
use crate::{SolveApp, theme::Tokens, viewport::Proj};
use egui::{Pos2, Sense, Stroke};
use serde_json::json;
use solidcraft_engine::geom::{Mesh, Vec3};
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
mod context;
mod controls;
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
pub struct State {
    context: context::State,
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
    empty_slots: [SceneSlot; 3],
    uploaded: u64,
    pub(crate) projected: Vec<(usize, Pos2)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            context: Default::default(),
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
            empty_slots: std::array::from_fn(|_| Arc::new(Mutex::new(Some(GpuScene::default())))),
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
        app.forms_prototype.controls.tolerance = app.preferences.forms_cad_tolerance.to_string();
        controls::sync(app);
        panel::sync(app);
        controls::load(app);
        app.home.open = false;
        app.ui.show_browser = false;
        app.ui.show_timeline = false;
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
            let p = *curve.get(curve.len() / 2)?;
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
    json!({"active":app.session.forms_prototype.is_some(),"selected":app.forms_prototype.selected,"elements":elements,
        "edges":edges,
        "display":live_cad::inspect(app),"context":context::inspect(app),"tool":tool_panel::inspect(app),
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
    if matches!(id, "form.prototype.close" | "form.prototype.commit") {
        app.forms_prototype = State::default();
    }
    if id == "browser.visibility" {
        app.forms_prototype.context = Default::default();
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
        app.cam = solidcraft_engine::render::Camera { target: center, distance: bounds * 4.0, ..Default::default() };
        // An explicit Form fit also fulfils the initial application fit. Otherwise
        // an import completed before first layout is replaced by the empty model's view.
        app.fitted = true;
    }
}

pub(crate) fn background_tick(app: &mut SolveApp, ctx: &egui::Context) {
    interchange::tick(app, ctx);
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
    context::tick(app, ui.ctx());
    panel::show(app, ui);
    if app.session.forms_prototype.is_none() {
        return;
    }
    let busy = app.forms_prototype.pending.is_some() || app.forms_prototype.interchange.is_some();
    let token = Tokens::get();
    let rect = ui.available_rect_before_wrap();
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
    let painter =
        ui.painter().clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.id().with("forms_transform"))).with_clip_rect(rect);
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
        && !d.moved
    {
        selection::released(app, d.toggle);
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
        let drag = app.forms_prototype.drag.take().or_else(|| app.forms_prototype.gizmo_drag.take());
        crate::gizmo::cancel(ui.ctx(), ui.id().with("forms_transform_drag"));
        if drag.is_some_and(|d| d.moved) {
            run(app, "form.prototype.undo", json!({}));
        }
        app.forms_prototype.setting_pivot = false;
    }
    ui.painter().rect_filled(rect, 0.0, token.viewport_top);
    app.forms_prototype.hover = input.hover.filter(|_| input.inside && !gizmo_hot && !tool_active).and_then(|p| surface::nearest(app, &proj, p));
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
            Pick::Vertex => form.logical.faces.iter().flatten().copied().map(Element::Vertex).collect(),
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
        let z = ids.iter().filter_map(|&i| form.surface.mesh.normals.get(i)).copied().fold(Vec3::ZERO, |a, b| a + b).normalized().unwrap_or(Vec3::Z);
        let edge = ids.windows(2).find_map(|p| Some(*form.surface.vertices.get(p[1])? - *form.surface.vertices.get(p[0])?)).unwrap_or(Vec3::X);
        let x = (edge - z * z.dot(edge)).normalized().unwrap_or_else(|| z.any_perp().normalized().unwrap_or(Vec3::X));
        return [x, z.cross(x), z];
    }
    [Vec3::X, Vec3::Y, Vec3::Z]
}
fn triad(app: &SolveApp) -> Option<crate::gizmo::Triad> {
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
        && (tools::active(app) || tool_panel::active(app))
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
        if changed
            && run(app, if extrude { "form.prototype.extrude_drag" } else { command }, params)
            && let Some(d) = app.forms_prototype.gizmo_drag.as_mut()
        {
            d.moved = true;
        }
    }
    if event.done {
        app.forms_prototype.gizmo_drag = None;
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
        if run(app, "form.prototype.extrude_drag", params) {
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
fn extrusion_params(app: &SolveApp, delta: Vec3, checkpoint: bool) -> serde_json::Value {
    let faces: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).collect();
    let edges: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Edge(e) = e { Some(*e) } else { None }).collect();
    json!({"faces":faces,"edges":edges,"displacement":[delta.x,delta.y,delta.z],"checkpoint":checkpoint})
}

fn render(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj) {
    let Some(form) = &app.session.forms_prototype else { return };
    if app.viewport.gpu.is_none() {
        for entry in &app.forms_prototype.context.entries {
            fallback(ui, proj, &entry.mesh);
        }
        fallback(ui, proj, live_cad::mesh(app).unwrap_or(&form.surface.mesh));
        return;
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    form.levels.hash(&mut hash);
    app.forms_prototype.context.version.hash(&mut hash);
    context::key(app).hash(&mut hash);
    controls::operation(app).hash(&mut hash);
    form.revision.hash(&mut hash);
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
        let selected = scene.color(Tokens::get().sel_face.to_array());
        let hovered = scene.color(Tokens::get().body_selected.to_array());
        let faces = surface::selected_faces(app);
        let mesh = live_cad::mesh(app).unwrap_or(&form.surface.mesh);
        for (ti, triangle) in mesh.triangles.iter().enumerate() {
            let root = mesh.tri_face.get(ti).copied().unwrap_or(u32::MAX) as usize;
            let c = if faces.contains(&root) {
                selected
            } else if app.forms_prototype.hover == Some(Element::Face(root))
                || (form.profile.is_some() && app.forms_prototype.hover == Some(Element::Surface))
            {
                hovered
            } else {
                color
            };
            for &i in triangle {
                if let (Some(p), Some(n)) = (mesh.positions.get(i as usize), mesh.normals.get(i as usize)) {
                    scene.tri(p.to_f32(), n.to_f32(), c);
                }
            }
        }
        let edge_color = scene.color(Tokens::get().body_edge.to_array());
        if live_cad::mesh(app).is_some() {
            for (i, curve) in mesh.edges.iter().enumerate() {
                if !mesh.seams.get(i).copied().unwrap_or(false) {
                    for pair in curve.windows(2) {
                        scene.line(pair[0].to_f32(), pair[1].to_f32(), edge_color, 1.4, false);
                    }
                }
            }
        } else {
            for (_, curve) in &form.surface.boundaries {
                for pair in curve.windows(2) {
                    scene.line(pair[0].to_f32(), pair[1].to_f32(), edge_color, 1.4, false);
                }
            }
        }
        if app.forms_prototype.wire {
            for t in &form.surface.mesh.triangles {
                for [a, b] in [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]] {
                    if let (Some(a), Some(b)) = (form.surface.mesh.positions.get(a as usize), form.surface.mesh.positions.get(b as usize)) {
                        scene.line(a.to_f32(), b.to_f32(), edge_color, 0.6, false);
                    }
                }
            }
        }
        *app.forms_prototype.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(scene);
        app.forms_prototype.uploaded = key;
    }
    let frame = viewport_frame(app, proj, ui.ctx().pixels_per_point(), key);
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(proj.rect, ViewportCallback(frame)));
}
fn viewport_frame(app: &SolveApp, proj: &Proj, ppp: f32, key: u64) -> Frame {
    let slot = app.forms_prototype.slot.clone();
    let [sketch, highlight, preview] = app.forms_prototype.empty_slots.clone();
    let mut empty_hash = std::collections::hash_map::DefaultHasher::new();
    "solidcraft-forms-empty-overlays".hash(&mut empty_hash);
    let empty_key = empty_hash.finish();
    let eye = app.cam.eye();
    let rect = proj.rect;

    Frame {
        product_lights: None,
        product: false,
        key,
        slot: slot.clone(),
        sketch_key: empty_key,
        sketch_slot: sketch,
        hl_key: empty_key,
        hl_slot: highlight,
        pv_key: empty_key,
        pv_slot: preview,
        pv_hide: false,
        view_proj: proj.vp.to_f32(),
        back: app.cam.back().to_f32(),
        size_px: [rect.width() * ppp, rect.height() * ppp],
        clip: [0.0; 4],
        cap: [0.0; 4],
        images: Vec::new(),
        points: Vec::new(),
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
