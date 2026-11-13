//! Opt-in local Forms editing experiment, inside SolidCraft's normal viewport shell.
use crate::gpu::{Frame, GpuScene, SceneSlot};
use crate::{SolveApp, viewport::Proj};
use egui::{Pos2, Sense, Stroke};
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::forms_prototype::{FormTargetSpace, FormTransformStamp};
use solidcraft_engine::geom::Mesh;
use solidcraft_engine::geom::prelude::*;
use solidcraft_theme::Tokens;
use solidcraft_viewport::Camera;
use solidcraft_viewport_egui::ViewportCallback;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
mod combine_preview;
mod context;
mod controls;
mod curves;
mod diagnostics;
mod extrusion;
pub mod face_pick;
mod face_press;
mod finish;
mod interchange;
mod live_cad;
mod panel;
#[cfg(not(target_arch = "wasm32"))]
mod region_geometry;
mod selection;
mod soft_selection;
mod surface;
mod tool_panel;
mod tools;
mod transform_feedback;
mod transform_job;
mod translation_snap;
#[cfg(target_arch = "wasm32")]
pub use combine_preview::{BrowserPreview, BrowserPreviewHost, BrowserPreviewOutcome};
#[cfg(target_arch = "wasm32")]
pub use diagnostics::{BrowserCheckHost, BrowserCheckOutcome};
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
const WORLD_AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
impl Axis {
    fn vector(self, axes: [Vec3; 3]) -> Option<Vec3> {
        match self {
            Self::View => None,
            Self::X => Some(axes[0]),
            Self::Y => Some(axes[1]),
            Self::Z => Some(axes[2]),
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
    constraint_axes: [Vec3; 3],
    moved: bool,
    authored: bool,
    source: TransformSource,
    job_source: Option<FormTransformStamp>,
    basis: [Vec3; 3],
    scale: bool,
    length: f64,
    control: bool,
    weights: Vec<f64>,
    influence: std::collections::BTreeMap<usize, f64>,
    extrude: bool,
    toggle: Option<Element>,
    snap: Option<translation_snap::Gesture>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct TransformSource {
    document: u64,
    authoring: u64,
    form: (u64, u64),
}
impl TransformSource {
    fn current(app: &SolveApp) -> Option<Self> {
        Some(Self {
            document: app.session.uid,
            authoring: app.session.authoring_revision(),
            form: app.session.forms_prototype.as_ref()?.source_revision(),
        })
    }
}
impl Drag {
    fn record_move(&mut self, source: Option<TransformSource>) {
        self.moved = true;
        if let Some(source) = source {
            self.authored |= self.source.form != source.form;
            self.source = source;
        }
    }
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
    diagnostics: diagnostics::State,
    extrusion: extrusion::State,
    #[cfg(not(target_arch = "wasm32"))]
    face_press: face_press::State,
    transform_job: transform_job::State,
    feedback: Option<transform_feedback::Feedback>,
    other_preview: Option<u64>,
    matching_preview: bool,
    selected: BTreeSet<usize>,
    elements: BTreeSet<Element>,
    selection: selection::State,
    soft: soft_selection::State,
    region: Option<selection::RegionDrag>,
    region_jobs: selection::RegionJobs,
    translation_snap: translation_snap::State,
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
    #[cfg(all(test, not(target_arch = "wasm32")))]
    finish_worker_gate: std::sync::Arc<std::sync::atomic::AtomicBool>,
    interchange: Option<interchange::Pending>,
    blend_surface: Option<bool>,
    live: live_cad::State,
    tools: tools::State,
    tool_panel: Option<tool_panel::State>,
    slot: SceneSlot,
    // Sketch, face highlights and preview: only the middle slot has Form geometry.
    overlay_slots: [SceneSlot; 3],
    uploaded: u64,
    /// Logical ID, evaluated world point and screen position from the same paint.
    pub(crate) projected: Vec<(usize, Vec3, Pos2)>,
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
            diagnostics: Default::default(),
            extrusion: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            face_press: Default::default(),
            transform_job: Default::default(),
            feedback: None,
            other_preview: None,
            matching_preview: false,
            selected: BTreeSet::new(),
            elements: BTreeSet::new(),
            selection: Default::default(),
            soft: Default::default(),
            region: None,
            region_jobs: Default::default(),
            translation_snap: Default::default(),
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
            #[cfg(all(test, not(target_arch = "wasm32")))]
            finish_worker_gate: Default::default(),
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
pub(crate) fn editing_feature(app: &SolveApp, id: u64) -> bool {
    app.session.forms_prototype.as_ref().is_some_and(|form| form.editing_feature == Some(id))
        || app.forms_prototype.repair_offer.as_ref().is_some_and(|(feature, _)| *feature == id)
        || app.forms_prototype.repair_entry.as_ref().is_some_and(|entry| entry.feature == id)
}
pub(crate) fn opened(app: &mut SolveApp) {
    if app.session.forms_prototype.is_some() {
        app.forms_prototype = State::default();
        app.forms_prototype.controls.tolerance = format!("{} mm", app.preferences.forms_cad_tolerance);
        controls::sync(app);
        panel::sync(app);
        controls::load(app);
        app.home.open = false;
        home(app);
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
        "edges":edges,"extrusion":extrusion::inspect(app),"transform_job":transform_job::inspect(app),"region_selection":selection::inspect_region(app),"transform_feedback":transform_feedback::inspect(app),
        "display":live_cad::inspect(app),"diagnostics":diagnostics::inspect(app),"source_curves":curves::inspect(app),"context":context::inspect(app),"tool":tool_panel::inspect(app),
        "combination":{"ready":combine_preview::ready(app).is_some(),"busy":combine_preview::busy(app),
            "error":combine_preview::result(app).and_then(|r|r.as_ref().err())},
        "vertices":app.forms_prototype.projected.iter().map(|(i,world,p)|json!({"index":i,"world":[world.x,world.y,world.z],"screen":[p.x,p.y]})).collect::<Vec<_>>()})
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
    frame(app, app.cam);
}

pub(crate) fn home(app: &mut SolveApp) {
    frame(app, Camera::default());
}

fn frame(app: &mut SolveApp, camera: Camera) {
    if let Some(f) = &app.session.forms_prototype {
        let points = f.control_positions();
        let center = points.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / points.len().max(1) as f64;
        let bounds: f64 = points.iter().map(|p| (*p - center).len()).fold(0.01, f64::max);
        app.cancel_view_animation();
        app.pending_fit = None;
        app.cam = Camera { target: center, distance: bounds * 4.0, ..camera };
        // An explicit Form fit also fulfils the initial application fit. Otherwise
        // an import completed before first layout is replaced by the empty model's view.
        app.fitted = true;
    }
}

pub(crate) fn leave_document(app: &mut SolveApp) {
    face_press::cancel(app);
    selection::cancel_region_work(app);
    extrusion::suspend(app);
    transform_job::suspend(app);
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
    selection::drain_region(app);
    repair_entry_tick(app);
    interchange::tick(app, ctx);
    diagnostics::prepare(app, ctx);
    diagnostics::tick(app);
    extrusion::tick(app, ctx);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn validation_prepared(
    app: &SolveApp,
) -> (Option<solidcraft_engine::forms_prototype::validation::Stamp>, Option<Arc<solidcraft_engine::forms_prototype::validation::Input>>) {
    diagnostics::prepared(app)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn validation_preparation_error<'a>(
    app: &'a SolveApp,
    current: &solidcraft_engine::forms_prototype::validation::Stamp,
) -> Option<&'a str> {
    diagnostics::preparation_error_for(app, current)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn install_validation_input_for_test(app: &mut SolveApp, input: Arc<solidcraft_engine::forms_prototype::validation::Input>) {
    diagnostics::install_source(app, input);
}

pub(crate) fn repair_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((feature, error)) = app.forms_prototype.repair_offer.clone() else { return };
    let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::unique("forms_repair_offer"), "edit", app.tr("forms.repair.title"))
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
        || transform_job::released(app)
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
    face_press::before_input(app, ui);
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let drag_id = transform_drag_id(app, ui);
    if escape {
        cancel_transform_gesture(app, ui.ctx(), drag_id);
    } else if !extrusion::active(app) {
        transform_job::discard_stale(app);
        discard_stale_transforms(app, ui.ctx(), drag_id);
    }
    if transform_job::take_gizmo_reset(app) {
        crate::gizmo::cancel(ui.ctx(), drag_id);
    }
    let before = crate::viewport::projection(app, rect);
    let hit_position = ui.input(|i| {
        let press = i.pointer.primary_down().then(|| i.pointer.press_origin()).flatten();
        press.or_else(|| i.pointer.hover_pos())
    });
    let geometry_first =
        hit_position.is_some_and(|p| matches!(app.forms_prototype.pick, Pick::Vertex | Pick::Edge) && surface::nearest(app, &before, p).is_some());
    let insertion_active = tools::active(app);
    let tool_active = insertion_active || tool_panel::active(app);
    let selecting_region = app.forms_prototype.region.is_some() || face_press::pending(app);
    let gizmo_hot = !selecting_region
        && !geometry_first
        && !tool_active
        && hit_position.is_some_and(|p| oriented_triad(app).is_some_and(|(t, axes)| crate::gizmo::contains_oriented(&before, &t, p, axes)));
    let painter = ui.painter().clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("forms_transform")));
    let dragging_gizmo = if busy || tool_active || selecting_region || escape {
        false
    } else if geometry_first && app.forms_prototype.gizmo_drag.is_none() {
        ui.add_enabled_ui(false, |ui| {
            show_gizmo(app, ui, &painter, &before);
        });
        false
    } else {
        show_gizmo(app, ui, &painter, &before)
    };
    let response = ui.interact(rect, ui.scope_id().with("forms_prototype_view"), Sense::click());
    let input = crate::navigation::handle(app, ui, rect, &response, crate::navigation::NavCaps::MODEL);
    let proj = crate::viewport::projection(app, rect);
    let block_region_result = busy
        || tool_active
        || dragging_gizmo
        || app.forms_prototype.drag.is_some()
        || input.mode.is_some()
        || selection::region_lost_focus(app, ui.input(|i| i.focused))
        || escape
        || (input.primary_pressed && input.inside);
    selection::poll_region(app, ui.ctx(), &proj, block_region_result);
    let region_owned = selection::region_pointer(app, ui, &proj, &input, busy || tool_active || dragging_gizmo);
    if !region_owned
        && input.primary_pressed
        && input.inside
        && input.mode.is_none()
        && !gizmo_hot
        && !dragging_gizmo
        && !busy
        && !escape
        && !tool_active
        && let Some(pos) = input.hover
        && !face_press::direct(app, ui, &proj, &input)
    {
        begin_drag(app, &proj, pos, input.mods);
    }
    let replayed = face_press::tick(app, ui, &proj, &input, busy || dragging_gizmo || region_owned || escape);
    if let Some(pos) = input.hover
        && !replayed
        && input.primary_down
        && app.forms_prototype.region.is_none()
        && !region_owned
        && input.mode.is_none()
        && !dragging_gizmo
        && !busy
        && !escape
        && !tool_active
    {
        enable_shift_extrusion(app, input.mods.shift);
        pull_with_modifiers(app, pos, input.mods);
    }
    if !input.primary_down
        && !replayed
        && !escape
        && !busy
        && !tool_active
        && input.mode.is_none()
        && app.forms_prototype.drag.as_ref().is_some_and(|drag| !drag.extrude && drag.job_source.is_some())
        && let Some((pos, modifiers)) = ui.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers } => Some((*pos, *modifiers)),
                _ => None,
            })
        })
    {
        pull_with_modifiers(app, pos, modifiers);
    }
    if !input.primary_down
        && let Some(d) = app.forms_prototype.drag.take()
    {
        if !d.moved {
            selection::released(app, d.toggle);
        } else if d.extrude {
            if !extrusion::release(app) {
                select_extruded_edges(app);
            }
        } else {
            transform_job::release(app);
        }
    }
    if !input.primary_down && !escape {
        selection::release_blocked_drag(app);
    }
    if input.inside
        && !region_owned
        && !face_press::pending(app)
        && !tool_active
        && !busy
        && !escape
        && ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary))
        && let Some(Element::Edge(edge)) = input.hover.and_then(|p| surface::nearest(app, &proj, p))
    {
        app.forms_prototype.elements.insert(Element::Edge(edge));
        selection::expand(app, false);
    }
    if escape {
        selection::cancel_region_work(app);
        tool_panel::cancel(app);
        app.forms_prototype.setting_pivot = false;
    }
    // Process input first so a same-frame release, superseding target or
    // Escape wins over an older completed sample.
    transform_job::tick(app, ui.ctx());
    live_cad::restore_fixed_profile_handles(app);
    ui.painter().rect_filled(rect, 0.0, token.viewport_top);
    crate::gpu::paint_background_picture(app, ui.painter(), rect);
    let hover_blocked = busy
        || tool_active
        || gizmo_hot
        || dragging_gizmo
        || app.forms_prototype.region.is_some()
        || region_owned
        || face_press::pending(app)
        || app.forms_prototype.drag.is_some()
        || extrusion::active(app)
        || transform_job::active(app);
    app.forms_prototype.hover = input.hover.filter(|_| input.inside && !hover_blocked).and_then(|p| surface::nearest(app, &proj, p));
    render(app, ui, &proj);
    surface::overlay(app, ui, &proj);
    soft_selection::paint(app, ui, &proj);
    transform_feedback::paint(app, ui, &proj);
    selection::paint_region(app, ui);
    tool_panel::pointer(app, ui, &proj, &input);
    if insertion_active && !busy {
        tools::pointer(app, ui, &proj, &input);
    }
    crate::viewport::axis_view(app, ui, rect);
}
fn cancel_transform_gesture(app: &mut SolveApp, ctx: &egui::Context, id: egui::Id) {
    transform_feedback::clear(app);
    selection::cancel_press(app);
    crate::gizmo::cancel(ctx, id);
    let pointer = app.forms_prototype.drag.take();
    let gizmo = app.forms_prototype.gizmo_drag.take();
    let source = TransformSource::current(app);
    let cancelled_extrusion = extrusion::cancel(app);
    let cancelled_transform = transform_job::cancel(app);
    if !cancelled_extrusion && !cancelled_transform && pointer.into_iter().chain(gizmo).any(|drag| drag.authored && Some(drag.source) == source) {
        run(app, "form.prototype.undo", json!({}));
    }
}
fn discard_stale_transforms(app: &mut SolveApp, ctx: &egui::Context, id: egui::Id) {
    let source = TransformSource::current(app);
    let stale = |drag: &Drag| {
        Some(drag.source) != source
            || (!drag.extrude && !transform_job::active(app) && drag.job_source.as_ref().is_some_and(|stamp| !stamp.is_current(&app.session)))
    };
    let pointer_stale = app.forms_prototype.drag.as_ref().is_some_and(stale);
    let gizmo_stale = app.forms_prototype.gizmo_drag.as_ref().is_some_and(stale);
    if pointer_stale {
        app.forms_prototype.drag = None;
        selection::cancel_press(app);
        transform_feedback::clear(app);
    }
    if gizmo_stale {
        app.forms_prototype.gizmo_drag = None;
        crate::gizmo::cancel(ctx, id);
        transform_feedback::clear(app);
    }
}
fn select_all(app: &mut SolveApp) {
    app.forms_prototype.pivot = None;
    app.forms_prototype.selection.active = None;
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
pub(crate) fn shortcut(app: &mut SolveApp, ctx: &egui::Context, key: egui::Key, mods: egui::Modifiers) -> bool {
    if app.session.forms_prototype.is_none() {
        return false;
    }
    if (tools::active(app) || tool_panel::active(app) || transform_job::active(app))
        && (matches!(key, egui::Key::Delete | egui::Key::Backspace) || (mods.command && matches!(key, egui::Key::A | egui::Key::Z | egui::Key::Y)))
    {
        return true;
    }
    if mods.command && !mods.alt && !mods.shift && key == egui::Key::A {
        select_all(app);
        return true;
    }
    if mods.is_none() && matches!(key, egui::Key::Delete | egui::Key::Backspace) {
        if controls::delete_selected_cap(app, ctx) {
            return true;
        }
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
    if let Some(hit) = surface::nearest(app, proj, pos) {
        begin_drag_hit(app, proj, pos, mods, hit);
        return;
    }
    transform_feedback::clear(app);
    selection::cancel_region_work(app);
    if !app.forms_prototype.setting_pivot && selection::begin_region(app, proj, pos, mods) {
        return;
    }
    clear_empty_press(app, mods);
}

fn clear_empty_press(app: &mut SolveApp, mods: egui::Modifiers) {
    transform_feedback::clear(app);
    if !(mods.shift || mods.ctrl || mods.command) {
        app.forms_prototype.selected.clear();
        app.forms_prototype.elements.clear();
        app.forms_prototype.selection = Default::default();
        app.forms_prototype.pivot = None;
    }
}

fn begin_drag_hit(app: &mut SolveApp, proj: &Proj, pos: Pos2, mods: egui::Modifiers, hit: Element) {
    transform_feedback::clear(app);
    selection::cancel_region_work(app);
    let add = mods.shift || mods.ctrl || mods.command || app.preferences.forms_greedy_selection;
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
    let skin: Vec<_> = app.forms_prototype.selected.iter().filter_map(|&i| form.surface.vertices.get(i).copied()).collect();
    let anchor = skin.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / skin.len().max(1) as f64;
    let extrude = form.profile.is_none() && mods.shift && matches!(hit, Element::Face(_) | Element::Edge(_));
    selection::activate(app, hit);
    if surface::cap_selected(app) {
        app.forms_prototype.drag = None;
        selection::pressed(app, hit, mods.shift);
        selection::blocked_drag(app, toggle);
        app.set_status(app.tr("forms.fill.cap_rim_editing").to_owned(), false);
        return;
    }
    let dirs = match basis(app) {
        Ok(dirs) => dirs,
        Err(key) => {
            selection::pressed(app, hit, mods.shift);
            selection::blocked_drag(app, toggle);
            app.set_status(app.tr(key).to_owned(), true);
            return;
        }
    };
    let constraint_axes = constraint_axes(app, dirs);
    let targets = match soft_selection::targets(app, extrude) {
        Ok(targets) => targets,
        Err(error) => {
            app.set_status(error, true);
            return;
        }
    };
    let snap = match translation_snap::pointer(app, proj, app.forms_prototype.axis.vector(constraint_axes)) {
        Ok(snap) => snap,
        Err(error) => {
            app.forms_prototype.drag = None;
            app.set_status(error, true);
            return;
        }
    };
    let Some(source) = TransformSource::current(app) else { return };
    app.forms_prototype.drag = Some(Drag {
        indices: targets.indices,
        points: targets.points,
        start: pos,
        proj: *proj,
        anchor,
        axis: app.forms_prototype.axis,
        constraint_axes,
        moved: false,
        authored: false,
        source,
        job_source: transform_job::source(app),
        basis: dirs,
        scale: false,
        length: 1.0,
        control: targets.control,
        weights: targets.weights,
        influence: targets.influence,
        extrude,
        toggle,
        snap,
    });
    selection::pressed(app, hit, mods.shift);
}
fn basis(app: &SolveApp) -> Result<[Vec3; 3], &'static str> {
    if let Some(d) = &app.forms_prototype.gizmo_drag {
        return Ok(d.basis);
    }
    if app.forms_prototype.orientation == 1 {
        let (x, y, z) = app.cam.basis();
        return Ok([x, y, z]);
    }
    if app.forms_prototype.orientation != 2 {
        return Ok(WORLD_AXES);
    }
    let selected = &app.forms_prototype.elements;
    let element =
        if selected.len() == 1 { selected.first().copied() } else { selection::active(app) }.ok_or("forms.selection_frame.single_element")?;
    let exact = live_cad::selection_geometry(app).ok_or("forms.selection_frame.current_cad")?;
    exact.frame(element).ok_or("forms.selection_frame.unavailable")
}
fn constraint_axes(app: &SolveApp, dirs: [Vec3; 3]) -> [Vec3; 3] {
    if app.forms_prototype.orientation == 2 { dirs } else { WORLD_AXES }
}
fn triad(app: &SolveApp) -> Option<crate::gizmo::Triad> {
    if surface::cap_selected(app) || app.forms_prototype.selected.iter().any(|&id| !live_cad::handle_available(app, id)) {
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
fn oriented_triad(app: &SolveApp) -> Option<(crate::gizmo::Triad, [Vec3; 3])> {
    Some((triad(app)?, basis(app).ok()?))
}
pub(crate) fn owns_pointer(app: &SolveApp, proj: &Proj, pos: Pos2) -> bool {
    app.session.forms_prototype.is_some()
        && (face_press::owns_pointer(app)
            || surface::nearest(app, proj, pos).is_some()
            || oriented_triad(app).is_some_and(|(t, dirs)| crate::gizmo::contains_oriented(proj, &t, pos, dirs)))
}
pub(crate) fn owns_key(app: &SolveApp, key: egui::Key) -> bool {
    let tool_active = tools::active(app) || tool_panel::active(app);
    app.session.forms_prototype.is_some()
        && match key {
            egui::Key::Escape => {
                tool_active
                    || app.forms_prototype.drag.is_some()
                    || app.forms_prototype.gizmo_drag.is_some()
                    || extrusion::active(app)
                    || transform_job::active(app)
                    || selection::region_can_cancel(app)
            }
            egui::Key::M | egui::Key::Enter => tool_active,
            _ => false,
        }
}
fn transform_drag_id(app: &SolveApp, ui: &egui::Ui) -> egui::Id {
    ui.scope_id().with("forms_transform_drag").with(app.session.uid)
}
fn show_gizmo(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) -> bool {
    let Some((t, dirs)) = oriented_triad(app) else { return false };
    let id = transform_drag_id(app, ui);
    let shift_extrude = app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_none())
        && ui.input(|i| i.modifiers.shift)
        && app.forms_prototype.elements.iter().any(|e| matches!(e, Element::Face(_) | Element::Edge(_)));
    let scale = app.forms_prototype.scale && !shift_extrude;
    let Some(event) = crate::gizmo::show_mode(ui, painter, proj, id, &t, 0.0, (dirs, scale)) else {
        return app.forms_prototype.gizmo_drag.is_some();
    };
    if event.started || app.forms_prototype.gizmo_drag.is_none() {
        let Some(source) = TransformSource::current(app) else { return false };
        transform_feedback::clear(app);
        let targets = match soft_selection::targets(app, shift_extrude) {
            Ok(targets) => targets,
            Err(error) => {
                crate::gizmo::cancel(ui.ctx(), id);
                app.set_status(error, true);
                return true;
            }
        };
        let snap = match translation_snap::gizmo(
            app,
            proj,
            t.center,
            dirs,
            event.handle,
            !scale && matches!(event.change, crate::gizmo::Change::Translate(_)),
        ) {
            Ok(snap) => snap,
            Err(error) => {
                end_failed_snap_gizmo(app, ui.ctx(), id, error);
                return true;
            }
        };
        app.forms_prototype.drag = None;
        let (right, _, _) = app.cam.basis();
        let pixels = proj.to_screen(t.center).zip(proj.to_screen(t.center + right)).map_or(1.0, |(a, b)| a.distance(b).max(1e-6));
        app.forms_prototype.gizmo_drag = Some(Drag {
            indices: targets.indices,
            points: targets.points,
            start: Pos2::ZERO,
            proj: *proj,
            anchor: t.center,
            axis: Axis::View,
            constraint_axes: WORLD_AXES,
            moved: false,
            authored: false,
            source,
            job_source: transform_job::source(app),
            basis: dirs,
            scale,
            length: crate::gizmo::LEN_PX / f64::from(pixels),
            control: targets.control,
            weights: targets.weights,
            influence: targets.influence,
            extrude: shift_extrude,
            toggle: None,
            snap,
        });
    }
    if shift_extrude && app.forms_prototype.gizmo_drag.as_ref().is_some_and(|d| !d.moved && !d.extrude) {
        let snap = match translation_snap::gizmo(app, proj, t.center, dirs, event.handle, matches!(event.change, crate::gizmo::Change::Translate(_)))
        {
            Ok(snap) => snap,
            Err(error) => {
                end_failed_snap_gizmo(app, ui.ctx(), id, error);
                return true;
            }
        };
        if let Some(d) = app.forms_prototype.gizmo_drag.as_mut() {
            d.extrude = true;
            d.scale = false;
            d.snap = snap;
        }
    }
    if let Some(d) = &app.forms_prototype.gizmo_drag {
        let change = match event.change {
            crate::gizmo::Change::Translate(delta) if !d.scale => {
                match translation_snap::apply(d.snap.as_ref(), delta, crate::prefs::manipulator_free_held(app, ui.input(|i| i.modifiers))) {
                    Ok(delta) => crate::gizmo::Change::Translate(delta),
                    Err(error) => {
                        end_failed_snap_gizmo(app, ui.ctx(), id, error);
                        return true;
                    }
                }
            }
            other => other,
        };
        let readout = transform_feedback::gizmo_value(change, d, event.handle, &app.cam);
        if !d.extrude && transform_job::repeats(app, transform_job::Owner::Gizmo, readout) {
            if event.done
                && let Some(drag) = app.forms_prototype.gizmo_drag.take()
            {
                finish_gizmo_drag(app, drag);
            }
            return true;
        }
        let feedback_basis = d.basis;
        let feedback_soft = d.weights.iter().any(|&weight| weight > 0.0 && weight < 1.0);
        let positions: Option<Vec<[f64; 3]>> = d
            .points
            .iter()
            .zip(&d.weights)
            .map(|(&p, &weight)| {
                let q = match readout {
                    transform_feedback::Value::Scale(factors) => solidcore_ops::form_transform::scale(p, d.anchor, d.basis, factors, weight),
                    transform_feedback::Value::Move(delta) => solidcore_ops::form_transform::translate(p, delta, weight),
                    transform_feedback::Value::Rotate { axis, angle } => {
                        solidcore_ops::form_transform::rotate(p, d.anchor, d.basis[axis], angle, weight)?
                    }
                };
                Some([q.x, q.y, q.z])
            })
            .collect();
        let Some(positions) = positions else {
            let error = app.tr("forms.prototype.invalid_number").to_owned();
            end_failed_snap_gizmo(app, ui.ctx(), id, error);
            return true;
        };
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
        let space = if d.control { FormTargetSpace::Control } else { FormTargetSpace::Surface };
        let background = d.job_source.is_some();
        let extrude = d.extrude && matches!(change, crate::gizmo::Change::Translate(_));
        let params = if extrude {
            let crate::gizmo::Change::Translate(delta) = change else { return true };
            extrusion_params(app, delta, !d.moved)
        } else {
            json!({"vertices":d.indices,"positions":positions,"checkpoint":!d.authored})
        };
        if changed || (!extrude && transform_job::active(app)) {
            let applied = if extrude {
                let axis = match event.handle {
                    crate::gizmo::Handle::Axis(index) => feedback_basis.get(index).copied(),
                    _ => None,
                };
                extrusion::command(app, params, axis)
            } else if background {
                transform_job::submit(app, transform_job::Owner::Gizmo, space, readout, params)
            } else {
                Some(d.source) == TransformSource::current(app) && run(app, space.command(), params)
            };
            if applied {
                transform_feedback::record(app, readout, feedback_basis, feedback_soft);
                let source = if extrude { None } else { TransformSource::current(app) };
                if let Some(d) = app.forms_prototype.gizmo_drag.as_mut() {
                    d.record_move(source);
                }
                if extrude
                    && let Some(form) = &app.session.forms_prototype
                    && form.hierarchy.is_some()
                {
                    app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&form.logical)).collect();
                }
            } else {
                transform_feedback::clear(app);
            }
        }
    }
    if event.done
        && let Some(d) = app.forms_prototype.gizmo_drag.take()
    {
        finish_gizmo_drag(app, d);
    }
    true
}
fn end_failed_snap_gizmo(app: &mut SolveApp, ctx: &egui::Context, id: egui::Id, error: String) {
    transform_feedback::clear(app);
    let drag = app.forms_prototype.gizmo_drag.take();
    crate::gizmo::cancel(ctx, id);
    if let Some(drag) = drag {
        finish_gizmo_drag(app, drag);
    }
    app.set_status(error, true);
}
fn finish_gizmo_drag(app: &mut SolveApp, drag: Drag) {
    if !drag.extrude {
        transform_job::release(app);
    } else if drag.moved && !extrusion::release(app) {
        select_extruded_edges(app);
    }
}
fn displacement(d: &Drag, pos: Pos2) -> Option<Vec3> {
    if let Some(axis) = d.axis.vector(d.constraint_axes) {
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
#[cfg(test)]
fn pull(app: &mut SolveApp, pos: Pos2) {
    pull_with_modifiers(app, pos, egui::Modifiers::NONE);
}
fn enable_shift_extrusion(app: &mut SolveApp, shift: bool) {
    if shift
        && app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_none())
        && app.forms_prototype.elements.iter().any(|e| matches!(e, Element::Face(_) | Element::Edge(_)))
        && let Some(drag) = app.forms_prototype.drag.as_mut()
        && !drag.moved
    {
        drag.extrude = true;
    }
}
fn pull_with_modifiers(app: &mut SolveApp, pos: Pos2, mods: egui::Modifiers) {
    let Some(d) = app.forms_prototype.drag.as_ref() else { return };
    if !d.extrude && Some(d.source) != TransformSource::current(app) {
        return;
    }
    if pos.distance(d.start) < 2.0 && !d.moved {
        return;
    }
    let Some(delta) = displacement(d, pos) else { return };
    let delta = match translation_snap::apply(d.snap.as_ref(), delta, crate::prefs::manipulator_free_held(app, mods)) {
        Ok(delta) => delta,
        Err(error) => {
            transform_feedback::clear(app);
            app.set_status(error, true);
            return;
        }
    };
    // Do not author a zero-height extrusion or consume a click before the first grid step.
    if d.snap.is_some() && !d.moved && delta.len() < 1e-12 {
        return;
    }
    let feedback_basis = d.basis;
    let feedback_soft = d.weights.iter().any(|&weight| weight > 0.0 && weight < 1.0);
    if d.extrude {
        let params = extrusion_params(app, delta, !d.moved);
        if extrusion::command(app, params, d.axis.vector(d.constraint_axes)) {
            transform_feedback::record(app, transform_feedback::Value::Move(delta), feedback_basis, feedback_soft);
            if let Some(d) = app.forms_prototype.drag.as_mut() {
                d.moved = true;
            }
            if let Some(f) = &app.session.forms_prototype {
                app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&f.logical)).collect();
            }
        } else {
            transform_feedback::clear(app);
        }
        return;
    }
    let readout = transform_feedback::Value::Move(delta);
    if transform_job::repeats(app, transform_job::Owner::Pointer, readout) {
        return;
    }
    let positions: Vec<[f64; 3]> = d
        .points
        .iter()
        .zip(&d.weights)
        .map(|(&p, &weight)| {
            let q = solidcore_ops::form_transform::translate(p, delta, weight);
            [q.x, q.y, q.z]
        })
        .collect();
    let params = json!({"vertices":d.indices,"positions":positions,"checkpoint":!d.authored});
    let space = if d.control { FormTargetSpace::Control } else { FormTargetSpace::Surface };
    let applied = if d.job_source.is_some() {
        transform_job::submit(app, transform_job::Owner::Pointer, space, readout, params)
    } else {
        run(app, space.command(), params)
    };
    if applied {
        transform_feedback::record(app, readout, feedback_basis, feedback_soft);
        let source = TransformSource::current(app);
        if let Some(d) = app.forms_prototype.drag.as_mut() {
            d.record_move(source);
        }
    } else {
        transform_feedback::clear(app);
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
    json!({"faces":faces,"edges":edges,"displacement":[delta.x,delta.y,delta.z],"checkpoint":checkpoint,"surface":true})
}

fn render(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj) {
    let Some(form) = &app.session.forms_prototype else { return };
    let ghost = extrusion::preview(app);
    let combined = combine_preview::ready(app).is_some();
    if app.viewport.gpu.is_none() {
        let tokens = Tokens::get();
        for entry in context::render_entries(app) {
            let color = if entry.detached_companion { entry.display_color(&tokens) } else { tokens.body };
            fallback(ui, proj, &entry.mesh, color);
        }
        if !combined {
            fallback(ui, proj, ghost.map(|p| &p.mesh).or_else(|| live_cad::mesh(app)).unwrap_or(&form.surface.mesh), tokens.body);
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
    let tokens = Tokens::get();
    tokens.body.to_array().hash(&mut hash);
    tokens.body_selected.to_array().hash(&mut hash);
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
        slot,
        sketch_key: empty_key,
        sketch_slot: sketch,
        hl_key: key,
        hl_slot: highlight,
        pv_key: empty_key,
        pv_slot: preview,
        pv_hide: false,
        preview_bounds: None,
        view_proj: proj.vp.to_f32(),
        turned_depth: Some(proj.vp.turned_depth_f32()),
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
        ao_samples: solidcraft_viewport_gpu::view::AO_SAMPLES,
        ssaa: 1.0,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        hatch: Default::default(),
        sky: crate::viewport::sky_uniform(app.preferences.dark_parts_shading, app.preferences.viewport_material_finish, proj.cam.up),
        moving: None,
    }
}
fn fallback(ui: &egui::Ui, proj: &Proj, mesh: &Mesh, color: egui::Color32) {
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
                triangle.colored_vertex(point, color);
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
    fn form_fit_keeps_orbit_and_home_resets_it() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("form.prototype.create", json!({"primitive":"box"})).unwrap();
        app.cam = Camera { target: Vec3::new(100.0, 0.0, 0.0), distance: 600.0, yaw: -0.4, pitch: 0.7, roll: 0.3, fov: 0.5, ..Camera::default() };
        let orbit = app.cam;
        app.run("view.fit", json!({})).unwrap();
        assert_ne!(app.cam.target, orbit.target);
        assert_ne!(app.cam.distance, orbit.distance);
        let framed = Camera { target: app.cam.target, distance: app.cam.distance, ..orbit };
        assert_eq!(app.cam, framed);
        app.run("view.home", json!({})).unwrap();
        assert_eq!(app.cam, Camera { target: framed.target, distance: framed.distance, ..Camera::default() });
    }

    #[test]
    fn editing_a_component_form_keeps_its_occurrence_active_until_the_editor_closes() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        let created = app.run("component.create", json!({"name":"Form Component"})).unwrap();
        let component = app.session.active_component;
        let occurrence = created["occurrence"].as_u64().unwrap();
        assert_ne!(component, 0);
        app.run("form.prototype.create", json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let saved = app.run("form.prototype.commit", json!({"name":"Skin","tolerance":0.05})).unwrap();
        let feature = saved["feature"].as_u64().unwrap();
        app.run("component.activate", json!({"component":0})).unwrap();

        app.edit_feature_in(feature, Some((component, &[occurrence])));
        assert!(editing_feature(&app, feature));
        assert_eq!(app.session.active_component, component);
        app.return_from_feature_edit();
        assert_eq!(app.session.active_component, component);

        app.run("form.prototype.close", json!({})).unwrap();
        app.return_from_feature_edit();
        assert_eq!(app.session.active_component, 0);
    }

    fn settle_released_extrusion(h: &mut crate::scenario::Harness) {
        extrusion::flush_for_test(&mut h.app, &h.ctx).unwrap();
        assert!(!extrusion::active(&h.app), "the released gesture must finish before inspecting its source");
        h.frame();
    }

    #[test]
    fn live_feedback_reports_the_submitted_snapped_move_and_clears_stale_or_failed_edits() {
        let (mut app, proj, start, id) = snap_plane();
        app.preferences.language = "en".into();
        app.preferences.length_decimals = 2;
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "6 mm".into();
        let original = app.session.forms_prototype.as_ref().unwrap().surface.vertices[id];
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        pull(&mut app, proj.to_screen(original + Vec3::X * 1.3).unwrap());
        let shown = transform_feedback::current(&app).unwrap();
        assert!(matches!(shown.value,transform_feedback::Value::Move(delta) if delta.dist(Vec3::X*2.0)<1e-7));
        assert!(shown.soft);
        assert_eq!(shown.basis, app.forms_prototype.drag.as_ref().unwrap().basis);
        let text = transform_feedback::text(&app, shown);
        assert!(text.contains("2.00 mm"));
        assert!(!text.contains("1.30"));
        assert!(text.contains("soft selection"));
        assert!(app.session.forms_prototype.as_ref().unwrap().surface.vertices[id].dist(original + Vec3::X * 2.0) < 1e-6);
        let source = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        let inspected = inspect(&app)["transform_feedback"].clone();
        assert_eq!(inspected["request"]["kind"], "move");
        let delta = inspected["request"]["delta_mm"].as_array().unwrap();
        assert!((delta[0].as_f64().unwrap() - 2.0).abs() < 1e-7);
        assert_eq!(inspected["soft"], true);
        assert_eq!(inspected["text"], text);
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), source);
        let ctx = egui::Context::default();
        crate::fonts::setup_bundled_fonts(&ctx);
        let painted = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(proj.rect), ..Default::default() }, |ui| {
            transform_feedback::paint(&mut app, ui, &proj)
        });
        let mut texts = Vec::new();
        for clipped in painted.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Text(text) = shape {
                    texts.push(text.galley.text().to_owned())
                }
            });
        }
        assert!(texts.iter().any(|painted| painted == &text));
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), source);
        app.preferences.forms_drag_feedback = false;
        assert!(transform_feedback::current(&app).is_none());
        assert!(inspect(&app)["transform_feedback"].is_null());
        let output = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(proj.rect), ..Default::default() }, |ui| {
            transform_feedback::paint(&mut app, ui, &proj)
        });
        let mut disabled_text = Vec::new();
        for clipped in output.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                if let egui::Shape::Text(text) = shape {
                    disabled_text.push(text.galley.text().to_owned());
                }
            });
        }
        assert!(disabled_text.is_empty(), "the legacy display has no transform readout");
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), source);
        app.preferences.forms_drag_feedback = true;
        pull(&mut app, proj.to_screen(original + Vec3::X * 3.3).unwrap());
        assert!(transform_feedback::current(&app).is_some());
        app.session.execute("parameters.add", &json!({"name":"outside","expression":"2 mm"})).unwrap();
        assert!(transform_feedback::current(&app).is_none(), "external document edits invalidate the cached source stamp");
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        discard_stale_transforms(&mut app, &ctx, egui::Id::unique("forms_feedback_test_gizmo"));
        assert!(app.forms_prototype.drag.is_none());
        assert!(app.forms_prototype.feedback.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);

        // A command refusal must discard the last successful readout rather
        // than displaying an old value as if it had just been applied. A fresh
        // drag is needed because the stale gesture cannot submit a command.
        let current = app.session.forms_prototype.as_ref().unwrap().surface.vertices[id];
        begin_drag(&mut app, &proj, proj.to_screen(current).unwrap(), egui::Modifiers::NONE);
        pull(&mut app, proj.to_screen(current + Vec3::X * 1.3).unwrap());
        assert!(transform_feedback::current(&app).is_some());
        app.forms_prototype.drag.as_mut().unwrap().points[0] = Vec3::new(f64::NAN, 0.0, 0.0);
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        pull(&mut app, proj.to_screen(current + Vec3::X * 3.3).unwrap());
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert!(transform_feedback::current(&app).is_none());
        assert!(app.forms_prototype.feedback.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn live_extrusion_feedback_separates_queued_accepted_and_fitted_requests_and_cancels() {
        let (mut app, proj, _, _) = snap_plane();
        let ctx = egui::Context::default();
        app.preferences.language = "en".into();
        app.forms_prototype.pick = Pick::Face;
        app.forms_prototype.axis = Axis::Z;
        let form = app.session.forms_prototype.as_ref().unwrap();
        let source = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let face = 5;
        let center = form.logical.faces[face].iter().map(|&id| form.surface.vertices[id]).fold(Vec3::ZERO, |a, b| a + b) / 4.0;
        let start = proj.to_screen(center).unwrap();
        begin_drag(&mut app, &proj, start, egui::Modifiers::SHIFT);
        pull_with_modifiers(&mut app, proj.to_screen(center + Vec3::Z * 1.3).unwrap(), egui::Modifiers::SHIFT);
        let requested = transform_feedback::current(&app).unwrap();
        assert_eq!(requested.phase, transform_feedback::Phase::Requested);
        assert_eq!(inspect(&app)["transform_feedback"]["phase"], "requested");
        assert!(matches!(requested.value,transform_feedback::Value::Move(delta) if delta.dist(Vec3::Z*2.0)<1e-7));
        assert!(transform_feedback::text(&app, requested).starts_with("Request"));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source, "a requested target is not applied geometry");
        extrusion::flush_for_test(&mut app, &ctx).unwrap();
        assert_eq!(transform_feedback::current(&app).unwrap().phase, transform_feedback::Phase::Fitting);
        assert_eq!(inspect(&app)["transform_feedback"]["phase"], "fitting");
        let form = app.session.forms_prototype.as_ref().unwrap();
        let cap = form.logical.faces[face].iter().map(|&id| form.surface.vertices[id]).fold(Vec3::ZERO, |a, b| a + b) / 4.0;
        assert!(cap.dist(center + Vec3::Z * 2.0) < 1e-6);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        live_cad::install_for_test(&mut app);
        assert_eq!(transform_feedback::current(&app).unwrap().phase, transform_feedback::Phase::Accepted);
        assert_eq!(inspect(&app)["transform_feedback"]["phase"], "accepted");
        let accepted = app.session.forms_prototype.as_ref().unwrap().definition();
        assert!(extrusion::command(&mut app, json!({"faces":[face],"edges":[],"displacement":[0,0,1e12],"surface":true,"checkpoint":false}), None));
        transform_feedback::record(&mut app, transform_feedback::Value::Move(Vec3::Z * 1e12), [Vec3::X, Vec3::Y, Vec3::Z], false);
        extrusion::flush_for_test(&mut app, &ctx).unwrap();
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert!(app.forms_prototype.feedback.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), accepted);
        // A refused later sequence must not inherit the earlier target's
        // accepted status, even if the pointer repeats that same invalid value.
        transform_feedback::record(&mut app, transform_feedback::Value::Move(Vec3::Z * 1e12), [Vec3::X, Vec3::Y, Vec3::Z], false);
        assert!(transform_feedback::current(&app).is_none());
        pull_with_modifiers(&mut app, proj.to_screen(center + Vec3::Z * 3.3).unwrap(), egui::Modifiers::SHIFT);
        assert_eq!(transform_feedback::current(&app).unwrap().phase, transform_feedback::Phase::Requested);
        assert!(extrusion::release(&mut app));
        app.forms_prototype.drag = None;
        assert!(transform_feedback::current(&app).is_some(), "released pending work retains its requested value");
        extrusion::flush_for_test(&mut app, &ctx).unwrap();
        assert!(transform_feedback::current(&app).is_none());
        let form = app.session.forms_prototype.as_ref().unwrap();
        let cap = form.logical.faces[face].iter().map(|&id| form.surface.vertices[id]).fold(Vec3::ZERO, |a, b| a + b) / 4.0;
        assert!(cap.dist(center + Vec3::Z * 4.0) < 1e-6);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
        begin_drag(&mut app, &proj, start, egui::Modifiers::SHIFT);
        pull_with_modifiers(&mut app, proj.to_screen(center + Vec3::Z * 1.3).unwrap(), egui::Modifiers::SHIFT);
        assert!(extrusion::cancel(&mut app));
        assert!(transform_feedback::current(&app).is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn live_feedback_keeps_joined_mirror_request_distinct_from_projected_cap_motion() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.preferences.language = "en".into();
        app.preferences.length_decimals = 2;
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[16,12,1],"segments":[4,3,1],"levels":1})).unwrap();
        app.session.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let source = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let face = form
            .logical
            .faces
            .iter()
            .position(|face| face.iter().all(|&id| form.logical.points[id].x >= 0.0) && face.iter().any(|&id| form.logical.points[id].x == 0.0))
            .unwrap();
        let mirrored = form.mirror_face_selection(&[face]).unwrap();
        let original_faces = form.logical.faces.clone();
        let original = form.surface.vertices.clone();
        app.forms_prototype.elements.insert(Element::Face(face));
        selection::sync(&mut app);
        let request = Vec3::new(0.5, 0.0, 2.0);
        let params = json!({"faces":[face],"edges":[],"displacement":[request.x,request.y,request.z],"surface":true,"checkpoint":true});
        let mut canonical = app.session.scratch();
        canonical.execute("form.prototype.extrude_drag", &params).unwrap();
        assert!(extrusion::command(&mut app, params, None));
        transform_feedback::record(&mut app, transform_feedback::Value::Move(request), [Vec3::X, Vec3::Y, Vec3::Z], false);
        let queued = transform_feedback::current(&app).unwrap();
        assert_eq!(queued.phase, transform_feedback::Phase::Requested);
        assert_eq!(queued.value, transform_feedback::Value::Move(request));
        assert!(transform_feedback::text(&app, queued).starts_with("Request"));
        extrusion::flush_for_test(&mut app, &egui::Context::default()).unwrap();
        let accepted = transform_feedback::current(&app).unwrap();
        assert_eq!(accepted.value, transform_feedback::Value::Move(request));
        let caption = transform_feedback::text(&app, accepted);
        assert!(caption.starts_with("Request accepted"));
        assert!(caption.contains("X 0.50 mm"));
        assert!(!caption.contains("Applied"));
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(
            form.definition(),
            canonical.forms_prototype.as_ref().unwrap().definition(),
            "HUD must not alter the canonical projected geometry"
        );
        for face in mirrored {
            for (&old, &new) in original_faces[face].iter().zip(&form.logical.faces[face]) {
                let motion = form.surface.vertices[new] - original[old];
                assert!(motion.dist(Vec3::Z * 2.0) < 1e-7, "joined component projects the requested X motion");
                assert!(motion.dist(request) > 0.49, "the request is not the measured cap displacement");
            }
        }
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        form.symmetry.as_ref().unwrap().validate_reflection(&form.cage).unwrap();
        let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        live_cad::install_for_test(&mut app);
        let ready = transform_feedback::current(&app).unwrap();
        assert_eq!(ready.phase, transform_feedback::Phase::Accepted);
        assert_eq!(ready.value, transform_feedback::Value::Move(request));
        assert!(transform_feedback::text(&app, ready).starts_with("Request accepted"));
        app.preferences.forms_drag_feedback = false;
        transform_feedback::record(&mut app, transform_feedback::Value::Move(request), [Vec3::X, Vec3::Y, Vec3::Z], false);
        assert!(transform_feedback::current(&app).is_none());
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
        assert!(extrusion::cancel(&mut app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }

    #[test]
    fn live_gizmo_feedback_reports_submitted_axis_plane_uniform_scale_and_snapped_rotation() {
        for (handle, hierarchical) in [("x", false), ("xy", false), ("ball", false), ("rz", false), ("x", true), ("rz", true)] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.app.preferences.language = "en".into();
            h.app.run("form.prototype.create", json!({"primitive":"face","dimensions":[20,20,1],"segments":[2,2,1],"levels":0})).unwrap();
            if hierarchical {
                h.app.session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
            }
            let form = h.app.session.forms_prototype.as_ref().unwrap();
            let undo = form.inspect()["undo"].as_u64().unwrap();
            let ids = if hierarchical {
                form.logical.faces.iter().find(|face| !face.is_empty() && face.iter().all(|&id| form.selectable_control(id))).unwrap().clone()
            } else {
                (0..form.logical.points.len()).collect()
            };
            h.app.forms_prototype.elements = ids.into_iter().map(Element::Vertex).collect();
            selection::sync(&mut h.app);
            h.app.forms_prototype.pick = Pick::Face;
            h.app.forms_prototype.scale = handle != "rz";
            h.app.cam.fov = 0.0;
            fit(&mut h.app);
            h.app.cam.fov = 0.0;
            h.frames(3);
            let triad = triad(&h.app).unwrap();
            let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
            let (right, _, _) = h.app.cam.basis();
            let pixels = proj.to_screen(triad.center).unwrap().distance(proj.to_screen(triad.center + right).unwrap());
            let length = crate::gizmo::LEN_PX / f64::from(pixels);
            let delta = match handle {
                "x" => Vec3::X * (length * 0.5),
                "xy" => (Vec3::X - Vec3::Y) * (length * 0.25),
                "ball" => right * (length * 0.25),
                _ => Vec3::ZERO,
            };
            let from = if handle == "rz" { json!({"gizmo":handle,"turn":35}) } else { json!({"gizmo":handle}) };
            let to = if handle == "rz" { json!({"gizmo":handle,"turn":82}) } else { json!({"gizmo":handle,"by":[delta.x,delta.y,delta.z]}) };
            let [x0, y0] = h.at(&from).unwrap();
            let [x1, y1] = h.at(&to).unwrap();
            let from = egui::pos2(x0 as f32, y0 as f32);
            let to = egui::pos2(x1 as f32, y1 as f32);
            h.frame_with(vec![egui::Event::PointerMoved(from)]);
            h.frame_with(crate::test_support::pointer(from, true));
            h.frame_with(vec![egui::Event::PointerMoved(to)]);
            #[cfg(not(target_arch = "wasm32"))]
            if hierarchical {
                assert_eq!(transform_feedback::current(&h.app).unwrap().phase, transform_feedback::Phase::Requested);
                assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
                transform_job::flush_for_test(&mut h.app, &h.ctx).unwrap();
                assert_ne!(transform_feedback::current(&h.app).unwrap().phase, transform_feedback::Phase::Requested);
            }
            let shown = transform_feedback::current(&h.app).unwrap();
            let inspected = inspect(&h.app)["transform_feedback"].clone();
            assert_eq!(inspected["request"]["kind"], if handle == "rz" { "rotate" } else { "scale" });
            assert_eq!(inspected["basis"], json!(shown.basis.map(|axis| [axis.x, axis.y, axis.z])));
            assert_eq!(inspected["text"], transform_feedback::text(&h.app, shown));
            let drag = h.app.forms_prototype.gizmo_drag.as_ref().unwrap();
            assert_eq!(shown.basis, drag.basis);
            assert!(!shown.soft);
            match (handle, shown.value) {
                ("x", transform_feedback::Value::Scale(f)) => {
                    assert!((f[0] - 1.5).abs() < 1e-5);
                    assert!((f[1] - 1.0).abs() < 1e-5);
                    assert!((f[2] - 1.0).abs() < 1e-5);
                }
                ("xy", transform_feedback::Value::Scale(f)) => {
                    assert!((f[0] - 1.25).abs() < 1e-5);
                    assert!((f[1] - 0.75).abs() < 1e-5);
                    assert!((f[2] - 1.0).abs() < 1e-5);
                }
                ("ball", transform_feedback::Value::Scale(f)) => assert!(f.iter().all(|f| (*f - 1.25).abs() < 1e-5)),
                ("rz", transform_feedback::Value::Rotate { axis, angle }) => {
                    assert_eq!(axis, 2);
                    assert!((angle.to_degrees() - 45.0).abs() < 1e-6);
                }
                _ => panic!("wrong transform readout for {handle}: {shown:?}"),
            }
            let form = h.app.session.forms_prototype.as_ref().unwrap();
            let positions = if drag.control { form.control_positions() } else { form.surface.vertices.clone() };
            for (&id, &original) in drag.indices.iter().zip(&drag.points) {
                let expected = match shown.value {
                    transform_feedback::Value::Scale(f) => {
                        let p = original - drag.anchor;
                        drag.anchor + Vec3::new(p.x * f[0], p.y * f[1], p.z * f[2])
                    }
                    transform_feedback::Value::Rotate { angle, .. } => {
                        let p = original - drag.anchor;
                        let (sin, cos) = angle.sin_cos();
                        drag.anchor + Vec3::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos, p.z)
                    }
                    transform_feedback::Value::Move(_) => panic!("scale and rotation fixtures must not report a move request"),
                };
                assert!(positions[id].dist(expected) < 1e-6, "{handle}: readout and applied geometry diverged");
            }
            assert_eq!(form.inspect()["undo"], undo + 1);
            h.frame_with(crate::test_support::key(egui::Key::Escape));
            assert!(transform_feedback::current(&h.app).is_none());
            assert!(inspect(&h.app)["transform_feedback"].is_null());
            h.app.forms_prototype = Default::default();
            assert!(transform_feedback::current(&h.app).is_none());
        }
    }

    fn snap_plane() -> (SolveApp, Proj, Pos2, usize) {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":0})).unwrap();
        fit(&mut app);
        app.cam.fov = 0.0;
        app.forms_prototype.pick = Pick::Vertex;
        app.forms_prototype.axis = Axis::X;
        app.forms_prototype.translation_snap.enabled = true;
        app.forms_prototype.translation_snap.increment = "2 mm".into();
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let id = 12;
        let start = proj.to_screen(app.session.forms_prototype.as_ref().unwrap().surface.vertices[id]).unwrap();
        assert_eq!(surface::nearest(&app, &proj, start), Some(Element::Vertex(id)));
        (app, proj, start, id)
    }
    #[test]
    fn translation_snap_pointer_freezes_units_weights_and_one_undo_step() {
        let (mut app, proj, start, id) = snap_plane();
        app.forms_prototype.translation_snap.increment = "0.2 cm".into();
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "6 mm".into();
        let source = app.session.forms_prototype.as_ref().unwrap().definition();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        let drag = app.forms_prototype.drag.as_ref().unwrap();
        let indices = drag.indices.clone();
        let points = drag.points.clone();
        let weights = drag.weights.clone();
        assert!(weights.iter().any(|&w| w > 0.0 && w < 1.0));
        let from = app.session.forms_prototype.as_ref().unwrap().surface.vertices[id];
        let at = |distance| proj.to_screen(from + Vec3::X * distance).unwrap();
        pull_with_modifiers(&mut app, at(0.4), egui::Modifiers::NONE);
        assert!(!app.forms_prototype.drag.as_ref().unwrap().moved);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        pull_with_modifiers(&mut app, at(1.3), egui::Modifiers::NONE);
        // Programmatic editing of the field does not replace the frozen gesture increment.
        app.forms_prototype.translation_snap.increment = "100 mm".into();
        pull_with_modifiers(&mut app, at(3.3), egui::Modifiers::NONE);
        let form = app.session.forms_prototype.as_ref().unwrap();
        for ((&vertex, &p), &w) in indices.iter().zip(&points).zip(&weights) {
            assert!(form.surface.vertices[vertex].dist(p + Vec3::X * (4.0 * w)) < 1e-6);
        }
        assert_eq!(form.inspect()["undo"].as_u64(), before["undo"].as_u64().map(|n| n + 1));
        pull_with_modifiers(&mut app, at(3.3), egui::Modifiers::COMMAND);
        assert!(app.session.forms_prototype.as_ref().unwrap().surface.vertices[id].dist(from + Vec3::X * 3.3) < 1e-6);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }
    #[test]
    fn translation_snap_respects_existing_free_modifier_preference_and_shift() {
        let (mut app, proj, _, _) = snap_plane();
        let gesture = translation_snap::pointer(&app, &proj, Some(Vec3::X)).unwrap().unwrap();
        let delta = Vec3::X * 3.3;
        let alt = egui::Modifiers::ALT;
        assert_eq!(translation_snap::apply(Some(&gesture), delta, crate::prefs::manipulator_free_held(&app, alt)).unwrap(), delta);
        app.preferences.manipulator_free_alt = false;
        assert_eq!(translation_snap::apply(Some(&gesture), delta, crate::prefs::manipulator_free_held(&app, alt)).unwrap(), Vec3::X * 4.0);
        assert_eq!(
            translation_snap::apply(Some(&gesture), delta, crate::prefs::manipulator_free_held(&app, egui::Modifiers::SHIFT)).unwrap(),
            Vec3::X * 4.0
        );
        assert_eq!(
            translation_snap::apply(Some(&gesture), delta, crate::prefs::manipulator_free_held(&app, egui::Modifiers::COMMAND)).unwrap(),
            delta
        );
    }
    #[test]
    fn translation_snap_gizmo_plane_keeps_view_orientation_and_soft_weights() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":0})).unwrap();
        h.app.cam.fov = 0.0;
        h.app.cam.roll = 0.35;
        h.app.forms_prototype.elements.insert(Element::Vertex(12));
        selection::sync(&mut h.app);
        h.app.forms_prototype.pick = Pick::Face;
        h.app.forms_prototype.orientation = 1;
        h.app.forms_prototype.soft.enabled = true;
        h.app.forms_prototype.soft.radius = "6 mm".into();
        h.app.forms_prototype.translation_snap.enabled = true;
        h.app.forms_prototype.translation_snap.increment = "2 mm".into();
        h.frames(3);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        let source = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let targets = soft_selection::targets(&h.app, false).unwrap();
        let dirs = basis(&h.app).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let from = crate::gizmo::handle_point("xy", Vec3::ZERO, 0.0).unwrap();
        let to = from + dirs[0] * 3.3 + dirs[1] * 1.3;
        let start = proj.to_screen(from).unwrap();
        let end = proj.to_screen(to).unwrap();
        h.call("ui.drag", json!({"x0":start.x,"y0":start.y,"x1":end.x,"y1":end.y,"steps":8}));
        crate::test_support::play(&mut h);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        let expected = dirs[0] * 4.0 + dirs[1] * 2.0;
        for ((&id, &p), &w) in targets.indices.iter().zip(&targets.points).zip(&targets.weights) {
            assert!(form.surface.vertices[id].dist(p + expected * w) < 1e-5, "{:?}", h.app.status);
        }
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        h.app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }

    #[test]
    fn fast_first_move_keeps_the_pressed_gizmo_axis() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("form.prototype.create", json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":0})).unwrap();
        h.app.cam.fov = 0.0;
        h.app.forms_prototype.elements.insert(Element::Vertex(12));
        selection::sync(&mut h.app);
        h.app.forms_prototype.pick = Pick::Face;
        h.frames(3);

        let form = h.app.session.forms_prototype.as_ref().unwrap();
        let original = form.surface.vertices[12];
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let from = crate::gizmo::handle_point("z", Vec3::ZERO, 0.0).unwrap();
        let start = proj.to_screen(from).unwrap();
        let distance = from.dist(original) * 0.5;
        let end = proj.to_screen(from + Vec3::Z * distance).unwrap();
        assert!(end.distance(start) > 12.0);
        assert!(crate::gizmo::contains_pointer(&proj, &triad(&h.app).unwrap(), start));
        assert!(!crate::gizmo::contains_pointer(&proj, &triad(&h.app).unwrap(), end));

        h.frame_with(vec![egui::Event::PointerMoved(start)]);
        h.frame_with(crate::test_support::pointer(start, true));
        h.frame_with(vec![egui::Event::PointerMoved(end)]);
        let shown = transform_feedback::current(&h.app).unwrap();
        assert!(matches!(shown.value, transform_feedback::Value::Move(delta) if delta.dist(Vec3::Z * distance) < 1e-5));
        h.frame_with(crate::test_support::pointer(end, false));
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        assert!(form.surface.vertices[12].dist(original + Vec3::Z * distance) < 1e-5);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
    }

    #[test]
    fn translation_snap_shift_extrusion_uses_the_same_increment_once() {
        let (mut app, proj, _, _) = snap_plane();
        let ctx = egui::Context::default();
        app.forms_prototype.pick = Pick::Face;
        app.forms_prototype.axis = Axis::Z;
        let form = app.session.forms_prototype.as_ref().unwrap();
        let source = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let face = 5;
        let center = form.logical.faces[face].iter().map(|&i| form.surface.vertices[i]).fold(Vec3::ZERO, |a, p| a + p) / 4.0;
        let start = proj.to_screen(center).unwrap();
        assert_eq!(surface::nearest(&app, &proj, start), Some(Element::Face(face)));
        begin_drag(&mut app, &proj, start, egui::Modifiers::SHIFT);
        assert!(app.forms_prototype.drag.as_ref().unwrap().extrude);
        pull_with_modifiers(&mut app, proj.to_screen(center + Vec3::Z * 1.3).unwrap(), egui::Modifiers::SHIFT);
        extrusion::flush_for_test(&mut app, &ctx).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.cage.faces.len(), source.cage.faces.len() + 4, "{:?}", app.status);
        let cap = form.logical.faces[face].iter().map(|&i| form.surface.vertices[i]).fold(Vec3::ZERO, |a, p| a + p) / 4.0;
        assert!((cap - center - Vec3::Z * 2.0).len() < 1e-6, "snapped cap={cap:?}, original={center:?}, status={:?}", app.status);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        assert!(extrusion::active(&app), "the first accepted displacement must retain the live gesture");
        pull_with_modifiers(&mut app, proj.to_screen(center + Vec3::Z * 3.3).unwrap(), egui::Modifiers::SHIFT);
        assert!(extrusion::release(&mut app));
        extrusion::flush_for_test(&mut app, &ctx).unwrap();
        assert!(!extrusion::active(&app));
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.cage.faces.len(), source.cage.faces.len() + 4, "updating the same gesture must not add another extrusion");
        let cap = form.logical.faces[face].iter().map(|&i| form.surface.vertices[i]).fold(Vec3::ZERO, |a, p| a + p) / 4.0;
        assert!((cap - center - Vec3::Z * 4.0).len() < 1e-6, "snapped cap={cap:?}, original={center:?}, status={:?}", app.status);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        let extruded = form.definition();
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
        app.session.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), extruded);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }
    #[test]
    fn translation_snap_invalid_input_is_atomic_and_off_retains_free_drag() {
        assert!(!translation_snap::State::default().enabled);
        let (mut app, proj, start, id) = snap_plane();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        for value in ["0 mm", "-1 mm", "not a length", "1e300 mm"] {
            app.forms_prototype.translation_snap.increment = value.into();
            begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
            assert!(app.forms_prototype.drag.is_none());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        }
        assert!(
            translation_snap::gizmo(&app, &proj, Vec3::ZERO, [Vec3::X, Vec3::Y, Vec3::Z], crate::gizmo::Handle::Ring(2), false).unwrap().is_none()
        );
        assert!(
            translation_snap::gizmo(&app, &proj, Vec3::ZERO, [Vec3::X, Vec3::Y, Vec3::Z], crate::gizmo::Handle::Axis(0), false).unwrap().is_none()
        );
        app.forms_prototype.translation_snap.enabled = false;
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        let from = app.session.forms_prototype.as_ref().unwrap().surface.vertices[id];
        pull_with_modifiers(&mut app, proj.to_screen(from + Vec3::X * 1.3).unwrap(), egui::Modifiers::NONE);
        assert!(app.session.forms_prototype.as_ref().unwrap().surface.vertices[id].dist(from + Vec3::X * 1.3) < 1e-6);
    }
    #[test]
    fn translation_snap_hierarchy_and_mirror_constraints_survive_pointer_undo() {
        let (mut app, _, _, _) = snap_plane();
        app.session.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        app.session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        fit(&mut app);
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let form = app.session.forms_prototype.as_ref().unwrap();
        let source = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let (id, point, start) = form
            .surface
            .vertices
            .iter()
            .enumerate()
            .filter(|(i, p)| form.selectable_control(*i) && p.x > 0.1)
            .find_map(|(id, &p)| {
                let at = proj.to_screen(p)?;
                (surface::nearest(&app, &proj, at) == Some(Element::Vertex(id))).then_some((id, p, at))
            })
            .unwrap();
        app.forms_prototype.axis = Axis::Z;
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        pull_with_modifiers(&mut app, proj.to_screen(point + Vec3::Z * 1.3).unwrap(), egui::Modifiers::NONE);
        #[cfg(not(target_arch = "wasm32"))]
        transform_job::flush_for_test(&mut app, &egui::Context::default()).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(form.surface.vertices[id].dist(point + Vec3::Z * 2.0) < 1e-6);
        form.symmetry.as_ref().unwrap().validate_reflection(&form.cage).unwrap();
        assert_eq!(form.hierarchy.as_ref().unwrap().leaves, source.hierarchy.as_ref().unwrap().leaves);
        assert_eq!(form.hierarchy.as_ref().unwrap().controls, source.hierarchy.as_ref().unwrap().controls);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn hierarchical_pointer() -> (SolveApp, Proj, Vec3, usize) {
        let (mut app, _, _, _) = snap_plane();
        app.session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        fit(&mut app);
        app.cam.fov = 0.0;
        app.forms_prototype.axis = Axis::Z;
        app.forms_prototype.translation_snap.enabled = false;
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let form = app.session.forms_prototype.as_ref().unwrap();
        let (id, point) = form
            .surface
            .vertices
            .iter()
            .copied()
            .enumerate()
            .find(|(id, point)| {
                form.selectable_control(*id)
                    && proj.to_screen(*point).is_some_and(|screen| surface::nearest(&app, &proj, screen) == Some(Element::Vertex(*id)))
            })
            .unwrap();
        begin_drag(&mut app, &proj, proj.to_screen(point).unwrap(), egui::Modifiers::NONE);
        (app, proj, point, id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn hierarchical_pointer_discards_a_superseded_worker_and_keeps_one_authored_checkpoint() {
        let (mut app, proj, point, vertex) = hierarchical_pointer();
        let ctx = egui::Context::default();
        let at = |distance| proj.to_screen(point + Vec3::Z * distance).unwrap();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        pull(&mut app, at(1.4));
        let (first, old_result) = transform_job::hold_for_test(&mut app).unwrap();
        pull(&mut app, at(3.4));
        assert!(old_result.send(first.compute().map_err(|error| error.to_string())).is_ok());
        assert_eq!(transform_feedback::current(&app).unwrap().phase, transform_feedback::Phase::Requested);
        transform_job::tick(&mut app, &ctx);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        transform_job::flush_for_test(&mut app, &ctx).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(form.surface.vertices[vertex].dist(point + Vec3::Z * 3.4) < 1e-5);
        assert_eq!(form.inspect()["undo"], undo + 1);
        assert!(app.forms_prototype.drag.as_ref().unwrap().authored);

        pull(&mut app, at(5.2));
        let (latest, result) = transform_job::hold_for_test(&mut app).unwrap();
        let latest = latest.compute().map_err(|error| error.to_string());
        cancel_transform_gesture(&mut app, &ctx, egui::Id::unique("cancel_pending_transform"));
        assert!(result.send(latest).is_err(), "Escape discards the pending result receiver");
        assert!(!transform_job::active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn released_hierarchical_transform_waits_for_a_permit_retained_by_an_abandoned_editor() {
        let (mut abandoned, proj, point, _) = hierarchical_pointer();
        let gate = abandoned.forms_prototype.transform_job.gate.clone();
        let permit = extrusion::WorkerPermit::acquire(gate.clone()).unwrap();
        pull(&mut abandoned, proj.to_screen(point + Vec3::Z * 1.4).unwrap());
        let (_, abandoned_result) = transform_job::hold_for_test(&mut abandoned).unwrap();
        abandoned.forms_prototype = Default::default();
        assert!(abandoned_result.send(Err("abandoned fixture".into())).is_err());

        let (mut app, proj, point, vertex) = hierarchical_pointer();
        app.forms_prototype.transform_job.gate = gate;
        let undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        let ctx = egui::Context::default();
        pull(&mut app, proj.to_screen(point + Vec3::Z * 2.4).unwrap());
        app.forms_prototype.drag = None;
        transform_job::release(&mut app);
        transform_job::tick(&mut app, &ctx);
        assert!(transform_job::released(&app));
        assert_eq!(transform_job::inspect(&app)["pending"], false);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        drop(permit);
        transform_job::flush_for_test(&mut app, &ctx).unwrap();
        assert!(!transform_job::active(&app));
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(form.surface.vertices[vertex].dist(point + Vec3::Z * 2.4) < 1e-5);
        assert_eq!(form.inspect()["undo"], undo + 1);
    }

    #[test]
    fn translation_snap_ball_and_oriented_planes_do_not_leak_normal_motion() {
        let (mut app, _, _, _) = snap_plane();
        app.cam.fov = 0.7;
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let center = Vec3::new(7.0, 3.0, 2.0);
        let normal = proj.ray(proj.to_screen(center).unwrap()).1;
        let (right, _, _) = proj.cam.basis();
        let x = (right - normal * right.dot(normal)).normalized().unwrap();
        let y = x.cross(normal).normalized().unwrap();
        let snap = translation_snap::gizmo(&app, &proj, center, [Vec3::X, Vec3::Y, Vec3::Z], crate::gizmo::Handle::Ball, true).unwrap().unwrap();
        let moved = translation_snap::apply(Some(&snap), x * 3.3 + y * 1.3, false).unwrap();
        assert!(moved.dist(x * 4.0 + y * 2.0) < 1e-9);
        assert!(moved.dot(normal).abs() < 1e-9);
        assert!(translation_snap::apply(Some(&snap), Vec3::new(f64::INFINITY, 0.0, 0.0), false).is_err());
        assert!(translation_snap::gizmo(&app, &proj, center, [Vec3::X, Vec3::Y, Vec3::Z], crate::gizmo::Handle::Axis(usize::MAX), true).is_err());
    }
    #[test]
    fn translation_snap_failed_gizmo_releases_pointer_and_preserves_last_undoable_edit() {
        let (mut app, proj, start, id) = snap_plane();
        let source = app.session.forms_prototype.as_ref().unwrap().definition();
        let point = app.session.forms_prototype.as_ref().unwrap().surface.vertices[id];
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        pull_with_modifiers(&mut app, proj.to_screen(point + Vec3::X * 1.3).unwrap(), egui::Modifiers::NONE);
        assert!(app.forms_prototype.drag.as_ref().unwrap().moved);
        app.forms_prototype.gizmo_drag = app.forms_prototype.drag.take();
        let accepted = app.session.forms_prototype.as_ref().unwrap().inspect();
        end_failed_snap_gizmo(&mut app, &egui::Context::default(), egui::Id::unique("snap_failure"), "invalid movement".into());
        assert!(app.forms_prototype.gizmo_drag.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), accepted);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }

    fn diagnostics_fixture() -> SolveApp {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        app
    }

    #[test]
    fn form_diagnostics_worker_feedback_is_read_only_and_source_qualified() {
        for change in [0, 1, 2, 3, 4] {
            let mut app = diagnostics_fixture();
            let result = diagnostics::compute_for_test(&app);
            assert!(result.is_ok());
            diagnostics::inject_for_test(&mut app, result);
            if change == 1 {
                app.session.execute("form.prototype.finish_options", &json!({"tolerance":"0.02 mm"})).unwrap();
            } else if change == 2 {
                app.session = solidcraft_engine::Session::default();
            } else if change == 3 {
                let point = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::Z;
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y,point.z]]})).unwrap();
            } else if change == 4 {
                app.session.execute("form.prototype.finish_options", &json!({"tolerance":"0.02 mm"})).unwrap();
                app.session.execute("form.prototype.finish_options", &json!({"tolerance":null})).unwrap();
            }
            let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
            diagnostics::tick_for_test(&mut app);
            assert_eq!(diagnostics::current_for_test(&app).is_some(), change == 0);
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
        }
    }

    #[test]
    fn form_diagnostics_distinguish_incomplete_coverage_and_keep_source_options_unchanged() {
        use solidcraft_engine::schema::forms_prototype::CheckStatus;
        let app = diagnostics_fixture();
        let mut report = diagnostics::compute_for_test(&app).unwrap();
        report.conversion.status = CheckStatus::Passed;
        report.record.status = CheckStatus::Passed;
        report.smlib.status = CheckStatus::Passed;
        assert_eq!(diagnostics::summary_for_test(&report), "forms.diagnostics.completed");
        for status in [CheckStatus::NotSupported, CheckStatus::NotRun] {
            report.smlib.status = status;
            assert_eq!(diagnostics::summary_for_test(&report), "forms.diagnostics.partial");
        }
        report.record.status = CheckStatus::Failed;
        assert_eq!(diagnostics::summary_for_test(&report), "forms.diagnostics.attention");
        // No diagnostics value is installed in the Form or its Finish options.
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().finish_options, Default::default());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
    }

    #[test]
    fn soft_weights_follow_authored_topology_and_pin_fixed_guides() {
        use std::collections::BTreeMap;
        let points = vec![Vec3::ZERO, Vec3::X, Vec3::X * 2.0, Vec3::Y * 0.1, Vec3::new(0.1, 0.1, 0.0), Vec3::new(0.2, 0.1, 0.0)];
        let faces = vec![vec![0, 1, 2], vec![3, 4, 5]];
        let seeds = [0].into_iter().collect();
        let editable = (0..6).collect();
        let weights = soft_selection::weights(&points, &faces, &seeds, &editable, &BTreeSet::new(), 2.0, 1).unwrap();
        assert_eq!(weights, BTreeMap::from([(0, 1.0), (1, 0.5)]), "nearby disconnected islands must not move");
        let locked = [1].into_iter().collect();
        let weights = soft_selection::weights(&points, &faces, &seeds, &editable, &locked, 3.0, 1).unwrap();
        assert!(!weights.contains_key(&1), "profile rim guides stay fixed");
        assert!(soft_selection::weights(&points, &faces, &seeds, &editable, &locked, f64::NAN, 0).is_err());
        let mut hostile = points.clone();
        hostile[1].x = f64::INFINITY;
        assert!(soft_selection::weights(&hostile, &faces, &seeds, &editable, &locked, 3.0, 0).is_err());
        assert!(soft_selection::weights(&points, &[vec![0, usize::MAX, 2]], &seeds, &editable, &locked, 3.0, 0).is_err());
    }
    #[test]
    fn soft_gesture_preserves_refinement_and_is_one_undo_step() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":1})).unwrap();
        app.session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let source = app.session.forms_prototype.as_ref().unwrap().definition();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let id = (0..form.surface.vertices.len()).find(|&i| form.selectable_control(i)).unwrap();
        app.forms_prototype.selected.insert(id);
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "15 mm".into();
        let targets = soft_selection::targets(&app, false).unwrap();
        assert!(!targets.control);
        assert!(targets.weights.iter().any(|&w| w < 1.0));
        let before_undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        for (step, distance) in [0.05, 0.1].into_iter().enumerate() {
            let positions: Vec<_> = targets
                .points
                .iter()
                .zip(&targets.weights)
                .map(|(&p, &w)| {
                    let p = solidcore_ops::form_transform::translate(p, Vec3::Z * distance, w);
                    [p.x, p.y, p.z]
                })
                .collect();
            app.session
                .execute("form.prototype.set_surface_vertices", &json!({"vertices":targets.indices,"positions":positions,"checkpoint":step == 0}))
                .unwrap();
        }
        let edited = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(edited.inspect()["undo"].as_u64().unwrap(), before_undo + 1);
        assert_eq!(edited.hierarchy.as_ref().unwrap().leaves, source.hierarchy.as_ref().unwrap().leaves);
        assert_eq!(edited.hierarchy.as_ref().unwrap().controls, source.hierarchy.as_ref().unwrap().controls);
        assert_ne!(edited.definition(), source);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }
    #[test]
    fn soft_mirror_targets_have_one_driver_and_engine_preserves_reflection() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":1})).unwrap();
        app.session.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let id = form.surface.vertices.iter().position(|p| p.x > 0.1).unwrap();
        app.forms_prototype.selected.insert(id);
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "100 mm".into();
        let targets = soft_selection::targets(&app, false).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let symmetry = form.symmetry.as_ref().unwrap();
        let ids: BTreeSet<_> = targets.indices.iter().copied().collect();
        for &i in &ids {
            let j = symmetry.partners[i];
            assert!(i == j || !ids.contains(&j));
            assert_eq!(targets.influence.get(&i), targets.influence.get(&j));
        }
        let positions: Vec<_> = targets
            .points
            .iter()
            .zip(&targets.weights)
            .map(|(&p, &w)| {
                let q = solidcore_ops::form_transform::translate(p, Vec3::Z * 0.1, w);
                [q.x, q.y, q.z]
            })
            .collect();
        app.session.execute("form.prototype.set_surface_vertices", &json!({"vertices":targets.indices,"positions":positions})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        form.symmetry.as_ref().unwrap().validate_reflection(&form.cage).unwrap();
    }
    #[test]
    fn sharp_selection_default_and_soft_extrusion_targets_are_unchanged() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({})).unwrap();
        app.forms_prototype.selected.insert(0);
        assert!(!app.forms_prototype.soft.enabled);
        let sharp = soft_selection::targets(&app, false).unwrap();
        assert_eq!(sharp.indices, vec![0]);
        assert_eq!(sharp.weights, vec![1.0]);
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "not a length".into();
        assert!(soft_selection::targets(&app, false).is_err());
        let extrude = soft_selection::targets(&app, true).unwrap();
        assert_eq!(extrude.indices, sharp.indices);
        assert_eq!(extrude.points, sharp.points);
        assert_eq!(extrude.weights, sharp.weights);
    }
    #[test]
    fn soft_pointer_drag_freezes_influence_and_keeps_one_undo_step() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":1})).unwrap();
        fit(&mut app);
        app.forms_prototype.pick = Pick::Vertex;
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "6 mm".into();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let pos = proj.to_screen(app.session.forms_prototype.as_ref().unwrap().surface.vertices[12]).unwrap();
        begin_drag(&mut app, &proj, pos, egui::Modifiers::NONE);
        let drag = app.forms_prototype.drag.as_ref().unwrap();
        assert!(drag.weights.iter().any(|&w| w > 0.0 && w < 1.0));
        let frozen = drag.weights.clone();
        let undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        // Even a programmatic settings change cannot reshape an already-running gesture.
        app.forms_prototype.soft.radius = "invalid during gesture".into();
        pull(&mut app, pos + egui::vec2(12.0, 0.0));
        pull(&mut app, pos + egui::vec2(24.0, 0.0));
        assert_eq!(app.forms_prototype.drag.as_ref().unwrap().weights, frozen);
        assert!(app.forms_prototype.drag.as_ref().unwrap().moved);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap(), undo + 1);
        assert_ne!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
    }
    #[test]
    fn oversized_soft_radius_refuses_without_source_history_or_selection_changes() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[16,16,1],"segments":[16,16,1],"levels":1})).unwrap();
        app.forms_prototype.selected.insert(0);
        app.forms_prototype.elements.insert(Element::Vertex(0));
        app.forms_prototype.soft.enabled = true;
        app.forms_prototype.soft.radius = "1000 mm".into();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        let selected = app.forms_prototype.elements.clone();
        assert!(soft_selection::targets(&app, false).is_err());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(app.forms_prototype.elements, selected);
    }
    #[test]
    fn soft_preview_key_tracks_worker_point_acceptance_without_authoring_changes() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({})).unwrap();
        let key = soft_selection::preview_key(&app);
        let authoring = app.session.authoring_revision();
        let form = app.session.forms_prototype.as_mut().unwrap();
        let revision = form.revision;
        Arc::make_mut(&mut form.surface).vertices[0] += Vec3::X * 0.01;
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().revision, revision);
        assert_eq!(app.session.authoring_revision(), authoring);
        assert_ne!(soft_selection::preview_key(&app), key);
    }
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
    fn compact_editor_keeps_insertion_controls_focused_and_legacy_layout_available() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.forms_compact_editor = true;
        h.app.run("form.prototype.create", json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        h.frames(3);
        let before = h.app.session.forms_prototype.as_ref().unwrap().inspect();
        let handle = crate::scenario::handle_at;
        let inserted_at = handle("forms_insert_points").unwrap();
        assert!(handle("forms_pick_vertex").unwrap().y < inserted_at.y);
        assert!(handle("forms_create_primitive").is_none());
        assert!(handle("forms_check_cad").is_none());
        assert!(handle("forms_refine_selected").is_some());

        crate::test_support::click(&mut h, "forms_insert_points");
        h.frames(2);
        assert!(handle("forms_pick_vertex").unwrap().y < handle("forms_insert_points").unwrap().y);
        assert!(handle("forms_commit_chain").is_some());
        assert!(handle("forms_insert_whole_loop").is_none());
        assert!(handle("forms_refine_selected").is_none());
        assert!(handle("forms.prototype.topology_section").is_none());

        let edge_button = handle("forms_insert_edge").unwrap();
        let cancel_button = handle("forms_cancel_insertion").unwrap();
        h.app.set_status("An exact-fit diagnostic changed the height of the status block", true);
        h.frames(2);
        assert_eq!(handle("forms_insert_edge"), Some(edge_button), "fit feedback must never move the insertion tabs");
        assert_eq!(handle("forms_cancel_insertion"), Some(cancel_button), "fit feedback must never move Cancel");
        crate::test_support::click(&mut h, "forms_insert_edge");
        h.frames(2);
        assert!(handle("forms_insert_whole_loop").is_some());
        assert!(handle("forms_commit_chain").is_none());
        assert!(handle("forms_refine_selected").is_none());
        crate::test_support::click(&mut h, "forms_cancel_insertion");
        h.frames(2);
        assert!(!tools::active(&h.app));
        assert!(handle("forms_refine_selected").is_some());
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), before);

        h.app.preferences.forms_compact_editor = false;
        h.frames(3);
        assert!(handle("forms_create_primitive").expect("legacy primitive controls default open").y < handle("forms_insert_points").unwrap().y);
        assert!(handle("forms_check_cad").is_some());
        h.app.preferences.forms_compact_editor = true;
        h.frames(3);
        assert!(handle("forms_create_primitive").is_none(), "the compact layout retains its folded primitive settings");
        h.app.preferences.forms_compact_editor = false;
        h.frames(3);
        assert!(handle("forms_create_primitive").is_some(), "the legacy layout retains its open primitive settings");
        crate::test_support::click(&mut h, "forms_insert_points");
        h.frames(2);
        assert!(handle("forms_refine_selected").is_some());
        assert!(handle("forms_cancel_insertion").is_none());
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
                        let outside = h.app.forms_prototype.projected.iter().any(|(_, _, p)| p.x > rect.right() && circle.center.distance(*p) < 0.1)
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
        let (index, _, visible) = h.app.forms_prototype.projected.iter().copied().find(|(_, _, p)| rect.shrink(30.0).contains(*p)).unwrap();
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
        assert!(curves::project(curve, &projection, outside).is_none(), "the exact hit threshold remains 9 px");
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
        assert!(h.app.prefs_window.shell.open);
        crate::prefs::reveal_preference(&mut h, "prefs:forms_drag_feedback");
        assert!(h.at(&json!({"handle":"prefs:forms_drag_feedback"})).is_ok());
        crate::prefs::reveal_preference(&mut h, "prefs:forms_preserve_selection");
        assert!(h.at(&json!({"handle":"prefs:forms_preserve_selection"})).is_ok());
        crate::prefs::reveal_preference(&mut h, "prefs:forms_mesh_while_dragging");
        assert!(h.at(&json!({"handle":"prefs:forms_mesh_while_dragging"})).is_ok());
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().definition(), original);
        h.call("ui.prefs", json!({"button":"cancel"}));
        h.frames(2);
        assert!(!h.app.prefs_window.shell.open);
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().definition(), original);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn exact_face_press_fixture() -> (crate::scenario::Harness, Vec3, Proj) {
        let mut h = crate::scenario::Harness::new();
        assert!(!h.app.preferences.forms_exact_face_picking);
        h.app.preferences.forms_exact_face_picking = true;
        h.app.run("form.prototype.create", json!({"primitive":"face","dimensions":[12,12,1],"segments":[1,1,1],"levels":0})).unwrap();
        h.app.cam.yaw = 0.0;
        h.app.cam.pitch = std::f64::consts::FRAC_PI_2;
        h.app.cam.distance = 24.0;
        h.app.cam.target = Vec3::ZERO;
        h.app.forms_prototype.pick = Pick::Face;
        live_cad::install_for_test(&mut h.app);
        h.frames(3);
        assert!(context::complete(&h.app));
        let projection = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let exact = live_cad::selection_geometry(&h.app).unwrap();
        let face = &exact.body.record().faces[0];
        let [[a, b], [c, d]] = face.domain;
        let chart = solidcraft_engine::geom::brep::eval::SurfaceEval::new(face.parameters()).unwrap();
        let point = chart.point([a + 0.37 * (b - a), c + 0.63 * (d - c)]).unwrap();
        (h, point, projection)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn exact_face_click_uses_one_released_press_and_preserves_the_source() {
        let (mut h, point, projection) = exact_face_press_fixture();
        let at = projection.to_screen(point).unwrap();
        h.app.set_status(h.app.tr("forms.face_pick.unresolved").to_owned(), true);
        let before = h.app.session.forms_prototype.as_ref().unwrap().inspect();
        let _permit = face_press::hold_for_test(&h.app).unwrap();
        let empty = egui::pos2(projection.rect.left() + 12.0, projection.rect.center().y);
        let mut press = crate::test_support::pointer(at, true);
        press.push(egui::Event::PointerMoved(empty));
        h.frame_with(press);
        assert!(face_press::pending(&h.app));
        assert!(h.app.forms_prototype.elements.is_empty());
        let answer = face_press::solve_for_test(&h.app).unwrap().unwrap();
        assert!(matches!(answer, face_pick::Answer::Hit { owner: face_pick::Owner::Face(0), .. }));
        let complete = face_press::response_for_test(&mut h.app).unwrap();
        h.frame_with(crate::test_support::pointer(at, false));
        complete.send(Ok(answer)).unwrap();
        h.frames(1);
        assert_eq!(h.app.forms_prototype.elements, BTreeSet::from([Element::Face(0)]));
        assert!(h.app.forms_prototype.drag.is_none());
        assert!(h.app.status.is_none(), "a qualified retry clears the previous exact-pick error");
        assert!(!face_press::pending(&h.app));
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_new_press_and_escape_discard_delayed_native_face_results() {
        let (mut h, point, projection) = exact_face_press_fixture();
        let at = projection.to_screen(point).unwrap();
        let before = h.app.session.forms_prototype.as_ref().unwrap().inspect();
        let _permit = face_press::hold_for_test(&h.app).unwrap();
        h.frame_with(crate::test_support::pointer(at, true));
        let answer = face_press::solve_for_test(&h.app).unwrap();
        let old = face_press::response_for_test(&mut h.app).unwrap();
        h.frame_with(crate::test_support::pointer(at, false));
        let next = at + egui::vec2(6.0, 0.0);
        h.frame_with(crate::test_support::pointer(next, true));
        old.send(answer).unwrap();
        h.frames(1);
        assert!(face_press::pending(&h.app));
        assert!(h.app.forms_prototype.elements.is_empty());
        let answer = face_press::solve_for_test(&h.app).unwrap();
        let latest = face_press::response_for_test(&mut h.app).unwrap();
        h.frame_with(crate::test_support::key(egui::Key::Escape));
        latest.send(answer).unwrap();
        h.frame_with(crate::test_support::pointer(next, false));
        assert!(!face_press::pending(&h.app));
        assert!(h.app.forms_prototype.elements.is_empty());
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().inspect(), before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_face_drag_replays_the_release_position_from_the_original_press() {
        let (mut h, point, projection) = exact_face_press_fixture();
        let start = projection.to_screen(point).unwrap();
        let end = projection.to_screen(point + Vec3::X * 2.0).unwrap();
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        let initial = form.surface.vertices[0];
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let _permit = face_press::hold_for_test(&h.app).unwrap();
        h.frame_with(crate::test_support::pointer(start, true));
        let answer = face_press::solve_for_test(&h.app).unwrap();
        let complete = face_press::response_for_test(&mut h.app).unwrap();
        h.frame_with(crate::test_support::pointer(end, false));
        assert_eq!(h.app.session.forms_prototype.as_ref().unwrap().surface.vertices[0], initial);
        complete.send(answer).unwrap();
        h.frames(1);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        assert!(form.surface.vertices[0].dist(initial + Vec3::X * 2.0) < 1e-5);
        assert_eq!(form.inspect()["undo"].as_u64(), Some(undo + 1));
        assert!(h.app.forms_prototype.drag.is_none());
    }
    // Exercise the real outer panel and pointer events while producer completion
    // is controlled explicitly; unrelated viewport polling must not race the
    // status transition whose original hit rectangle this test retains.
    fn draw_forms_panel(app: &mut SolveApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        crate::scenario::clear_handles();
        let _ = crate::test_support::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| panel::show(app, ui),
        );
    }
    #[test]
    fn primary_filter_rectangles_and_real_clicks_survive_source_status_changes() {
        for preserve in [true, false] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.preferences.language = "en".into();
            app.preferences.forms_preserve_selection = preserve;
            app.run("form.prototype.create", json!({"primitive":"face","dimensions":[8,8,1],"segments":[2,2,1],"levels":0})).unwrap();
            live_cad::install_for_test(&mut app);
            let curves = solidcraft_engine::forms_prototype::insertion_curves(&app.session, controls::tolerance(&app).unwrap(), &[]).unwrap();
            let mut completed = Some(curves::pending_for_test(&mut app));
            let ctx = egui::Context::default();
            crate::fonts::setup_bundled_fonts(&ctx);
            for _ in 0..3 {
                draw_forms_panel(&mut app, &ctx, Vec::new());
            }
            let source = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
            let handles = ["forms_pick_all", "forms_pick_vertex", "forms_pick_edge", "forms_pick_face", "forms_move_mode", "forms_scale_mode"];
            let mut moved_status = false;
            for (transition, handle, mode) in [
                ("curves-ready", "forms_pick_vertex", Pick::Vertex),
                ("cad-unavailable", "forms_pick_edge", Pick::Edge),
                ("cad-ready-curves-error", "forms_pick_all", Pick::All),
                ("curves-recovered", "forms_pick_face", Pick::Face),
            ] {
                app.forms_prototype.elements = BTreeSet::from([Element::Face(0)]);
                selection::sync(&mut app);
                let selected = app.forms_prototype.selected.clone();
                let pivot = Vec3::new(1.0, 2.0, 0.0);
                app.forms_prototype.pivot = Some(pivot);
                draw_forms_panel(&mut app, &ctx, Vec::new());
                let rectangles = handles.map(|name| crate::scenario::rect_of(name).unwrap());
                let old_at = crate::scenario::rect_of(handle).unwrap().center();
                let status_at = crate::scenario::handle_at("forms_fit_status").unwrap();
                // Resolve/hover before completion, then press that SAME position
                // after the actual producer states and wrapped messages change.
                draw_forms_panel(&mut app, &ctx, vec![egui::Event::PointerMoved(old_at)]);
                match transition {
                    "curves-ready" => {
                        completed.take().unwrap().send(Ok(curves.clone())).unwrap();
                        curves::poll_for_test(&mut app);
                        assert!(matches!(curves::ready(&app), Some(Ok(_))));
                    }
                    "cad-unavailable" => {
                        app.forms_prototype.live = Default::default();
                        assert!(live_cad::mesh(&app).is_none());
                    }
                    "cad-ready-curves-error" => {
                        live_cad::install_for_test(&mut app);
                        curves::install_for_test(&mut app, Err("Source curve preparation stopped before the current boundary guides could be made available. Retry after checking the Form source.".into()));
                        assert!(live_cad::mesh(&app).is_some());
                        assert!(matches!(curves::ready(&app), Some(Err(_))));
                    }
                    "curves-recovered" => curves::install_for_test(&mut app, Ok(curves.clone())),
                    _ => panic!("unknown status transition {transition}"),
                }
                draw_forms_panel(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerButton {
                        pos: old_at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                for (name, rectangle) in handles.into_iter().zip(rectangles) {
                    assert_eq!(
                        crate::scenario::rect_of(name),
                        Some(rectangle),
                        "{transition}, preserve={preserve}: asynchronous status moved {name}"
                    );
                }
                moved_status |= crate::scenario::handle_at("forms_fit_status").unwrap().y != status_at.y;
                draw_forms_panel(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerButton {
                        pos: old_at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert!(app.forms_prototype.pick == mode, "{transition}, preserve={preserve}: original-coordinate click missed its filter");
                if preserve {
                    assert_eq!(app.forms_prototype.elements, BTreeSet::from([Element::Face(0)]));
                    assert_eq!(app.forms_prototype.selected, selected);
                    assert_eq!(app.forms_prototype.pivot, Some(pivot));
                } else {
                    assert!(app.forms_prototype.elements.is_empty(), "{transition}: legacy filter click must clear entities");
                    assert!(app.forms_prototype.selected.is_empty());
                    assert!(app.forms_prototype.pivot.is_none());
                }
                assert_eq!(
                    solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(),
                    source,
                    "{transition}: filtering must not author source/history"
                );
            }
            assert!(moved_status, "the regression must exercise actual status layout movement below the stable targets");
        }
    }
    #[test]
    fn pinned_filter_controls_still_yield_to_tool_and_repair_modes() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.preferences.language = "en".into();
        app.run("form.prototype.create", json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        live_cad::install_for_test(&mut app);
        let ctx = egui::Context::default();
        crate::fonts::setup_bundled_fonts(&ctx);
        draw_forms_panel(&mut app, &ctx, Vec::new());
        assert!(crate::scenario::rect_of("forms_pick_vertex").is_some());
        assert!(crate::scenario::rect_of("forms_scale_mode").is_some());
        app.run("form.prototype.tool.begin", json!({})).unwrap();
        draw_forms_panel(&mut app, &ctx, Vec::new());
        assert!(tool_panel::active(&app));
        assert!(crate::scenario::rect_of("forms_pick_vertex").is_none());
        assert!(crate::scenario::rect_of("forms_scale_mode").is_none());
        app.run("form.prototype.tool.cancel", json!({})).unwrap();
        app.session.forms_prototype.as_mut().unwrap().repair = Some("Saved support requires reattachment".into());
        draw_forms_panel(&mut app, &ctx, Vec::new());
        assert!(crate::scenario::rect_of("forms_pick_vertex").is_none());
        assert!(crate::scenario::rect_of("forms_scale_mode").is_none());
        assert!(crate::scenario::handle_at("forms_repair_reattach").is_some());
    }
    #[test]
    fn filter_buttons_keep_selected_entities_pivot_and_history_while_the_filter_changes() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        h.app.preferences.forms_preserve_selection = true;
        h.app.run("form.prototype.create", json!({"primitive":"face","dimensions":[8,8,1],"segments":[2,2,1],"levels":0})).unwrap();
        h.app.cam.target = Vec3::ZERO;
        h.app.cam.distance = 18.0;
        h.app.cam.yaw = 0.0;
        h.app.cam.pitch = std::f64::consts::FRAC_PI_2;
        h.app.cam.fov = 0.0;
        live_cad::install_for_test(&mut h.app);
        h.frames(3);
        crate::test_support::click(&mut h, "forms_pick_face");
        let at_face = |h: &crate::scenario::Harness, id: usize| {
            let form = h.app.session.forms_prototype.as_ref().unwrap();
            let face = &form.logical.faces[id];
            let center = face.iter().map(|&vertex| form.surface.vertices[vertex]).fold(Vec3::ZERO, |sum, point| sum + point) / face.len() as f64;
            crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap()).to_screen(center).unwrap()
        };
        let point = at_face(&h, 0);
        h.call("ui.click", json!({"x":point.x,"y":point.y}));
        crate::test_support::play(&mut h);
        assert_eq!(h.app.forms_prototype.elements, BTreeSet::from([Element::Face(0)]));
        assert!(h.app.forms_prototype.selected.len() > 1, "one selected face contains multiple points");
        let entities = h.app.forms_prototype.elements.clone();
        let selected = h.app.forms_prototype.selected.clone();
        let pivot = Vec3::new(3.5, 3.5, 0.0);
        h.app.forms_prototype.pivot = Some(pivot);
        let source = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
        // Leave a range anchor and a press from the old filter pending. A filter
        // switch must discard them without changing the persistent selection.
        selection::pressed(&mut h.app, Element::Face(0), false);
        selection::released(&mut h.app, None);
        selection::pressed(&mut h.app, Element::Face(1), true);
        for (filter, mode) in [("vertex", Pick::Vertex), ("vertex", Pick::Vertex), ("edge", Pick::Edge), ("all", Pick::All), ("face", Pick::Face)] {
            crate::test_support::click(&mut h, &format!("forms_pick_{filter}"));
            assert!(h.app.forms_prototype.pick == mode);
            selection::released(&mut h.app, None);
            h.frames(1);
            assert_eq!(h.app.forms_prototype.elements, entities);
            assert_eq!(h.app.forms_prototype.selected, selected);
            assert_eq!(h.app.forms_prototype.pivot, Some(pivot));
            assert_eq!(panel::selection_summary(&h.app), "Selected: 1 face");
            assert!(h.widget("Selected: 1 face").is_some(), "the rendered summary describes selection, not the current filter");
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), source);
        }
        let point = at_face(&h, 1);
        h.call("ui.click", json!({"x":point.x,"y":point.y}));
        crate::test_support::play(&mut h);
        assert_eq!(h.app.forms_prototype.elements, BTreeSet::from([Element::Face(1)]));
        assert!(h.app.forms_prototype.pivot.is_none(), "an actual selection change still resets the pivot");
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), source);
    }
    #[test]
    fn legacy_filter_buttons_clear_selection_and_pivot_even_when_the_same_filter_is_clicked() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        h.app.run("form.prototype.create", json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        // Handle lookup, hover and press each advance the real UI. Qualify the
        // immutable producers before those frames: a newly wrapped CAD report
        // or disappearing source-curve status row above the filters must not
        // move a button between coordinate lookup and the actual press.
        live_cad::install_for_test(&mut h.app);
        let curves = solidcraft_engine::forms_prototype::insertion_curves(&h.app.session, controls::tolerance(&h.app).unwrap(), &[]).unwrap();
        curves::install_for_test(&mut h.app, Ok(curves));
        h.call("ui.prefs", json!({"forms_preserve_selection":false}));
        h.frames(3);
        assert!(!h.app.preferences.forms_preserve_selection);
        let source = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
        for (filter, mode) in [("vertex", Pick::Vertex), ("vertex", Pick::Vertex), ("edge", Pick::Edge), ("all", Pick::All), ("face", Pick::Face)] {
            h.app.forms_prototype.elements = BTreeSet::from([Element::Face(0)]);
            selection::sync(&mut h.app);
            h.app.forms_prototype.pivot = Some(Vec3::new(1.0, 2.0, 0.0));
            h.frames(1);
            assert!(live_cad::mesh(&h.app).is_some(), "filter {filter}: CAD fixture must remain qualified before the real click");
            assert!(matches!(curves::ready(&h.app), Some(Ok(_))), "filter {filter}: exact source curves must remain prepared");
            let handle = format!("forms_pick_{filter}");
            let at = crate::scenario::handle_at(&handle).unwrap();
            crate::test_support::click(&mut h, &handle);
            let current = match h.app.forms_prototype.pick {
                Pick::All => "all",
                Pick::Vertex => "vertex",
                Pick::Edge => "edge",
                Pick::Face => "face",
            };
            assert!(h.app.forms_prototype.pick == mode, "filter {filter} at {at:?}: real button click left current filter {current}");
            assert!(
                h.app.forms_prototype.elements.is_empty(),
                "filter {filter} at {at:?}, current {current}: legacy click retained entities {:?}",
                h.app.forms_prototype.elements
            );
            assert!(h.app.forms_prototype.selected.is_empty(), "filter {filter}, current {current}: legacy click retained expanded points");
            assert!(h.app.forms_prototype.pivot.is_none(), "filter {filter}, current {current}: legacy click retained the custom pivot");
            assert_eq!(panel::selection_summary(&h.app), "Nothing selected");
            assert!(h.widget("Nothing selected").is_some());
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), source);
        }
    }
    #[test]
    fn selection_summary_counts_mixed_entities_and_a_whole_surface_instead_of_its_points() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "en".into();
        h.app.run("form.prototype.create", json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        let face = h.app.session.forms_prototype.as_ref().unwrap().logical.faces[0].clone();
        let edge = [face[0].min(face[1]), face[0].max(face[1])];
        h.app.forms_prototype.elements = BTreeSet::from([Element::Vertex(face[2]), Element::Edge(edge), Element::Face(0)]);
        selection::sync(&mut h.app);
        h.app.forms_prototype.pick = Pick::Vertex;
        h.frames(3);
        assert_eq!(panel::selection_summary(&h.app), "Selected: 1 point, 1 edge, 1 face");
        assert!(h.widget("Selected: 1 point, 1 edge, 1 face").is_some());
        let next = h.app.session.forms_prototype.as_ref().unwrap().logical.faces[1].clone();
        h.app.forms_prototype.elements.extend([
            Element::Vertex(face[3]),
            Element::Edge([next[0].min(next[1]), next[0].max(next[1])]),
            Element::Face(1),
        ]);
        selection::sync(&mut h.app);
        h.frames(1);
        assert_eq!(panel::selection_summary(&h.app), "Selected: 2 points, 2 edges, 2 faces");
        assert!(h.widget("Selected: 2 points, 2 edges, 2 faces").is_some());
        h.app.run("form.prototype.close", json!({})).unwrap();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let body = h.app.session.model.state().bodies[0].clone();
        let top = (0..body.body.face_count()).find(|&id| body.body.planar_face(id).is_some_and(|plane| plane.normal().dot(Vec3::Z) > 0.99)).unwrap();
        h.app.run("form.surface", json!({"body":"Support","face":top,"segments":[2,2],"partition":false})).unwrap();
        live_cad::install_for_test(&mut h.app);
        h.app.forms_prototype.elements = BTreeSet::from([Element::Surface]);
        selection::sync(&mut h.app);
        assert!(h.app.forms_prototype.selected.len() > 1);
        h.frames(3);
        assert_eq!(panel::selection_summary(&h.app), "Selected: 1 surface");
        assert!(h.widget("Selected: 1 surface").is_some());
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
        let inspected = inspect(&h.app);
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        for &(index, world, p) in &positions {
            let handle = inspected["vertices"].as_array().unwrap().iter().find(|handle| handle["index"] == index).unwrap();
            assert_eq!(handle["world"], json!([world.x, world.y, world.z]));
            assert_eq!(handle["screen"], json!([p.x, p.y]));
            assert_eq!(proj.to_screen(world), Some(p), "inspection must report the exact point used to paint this handle");
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
        let (index, _, p) = positions[0];
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
            settle_released_extrusion(&mut h);
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
            settle_released_extrusion(&mut h);
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
        let ctx = egui::Context::default();
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        begin_drag(&mut app, &proj, start, egui::Modifiers { shift: true, ..Default::default() });
        for distance in [35.0, 70.0] {
            pull(&mut app, start + egui::vec2(distance, -20.0));
            extrusion::flush_for_test(&mut app, &ctx).unwrap();
            let f = app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(f.logical.faces.len(), 14, "six original faces and eight editable walls");
            assert_eq!(f.cage.faces.len(), 32, "24 refined source cells and eight wall cells");
            assert_eq!(f.hierarchy.as_ref().unwrap().base, cage);
            assert_eq!(f.inspect()["undo"], 1);
        }
        assert!(extrusion::release(&mut app));
        extrusion::flush_for_test(&mut app, &ctx).unwrap();
        assert!(!extrusion::active(&app));
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
        settle_released_extrusion(&mut h);
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
        let ctx = egui::Context::default();
        begin_drag(&mut app, &proj, start, egui::Modifiers { shift: true, ..Default::default() });
        for distance in [22.0, 44.0] {
            pull(&mut app, start + egui::vec2(distance, -15.0));
            extrusion::flush_for_test(&mut app, &ctx).unwrap();
            let f = app.session.forms_prototype.as_ref().unwrap();
            assert_eq!(f.cage.faces.len(), before.cage.faces.len() + 8, "{:?}", app.status);
            assert_eq!(f.inspect()["undo"], checkpoint_count + 1);
            f.symmetry.as_ref().unwrap().validate_reflection(&f.cage).unwrap();
        }
        assert!(extrusion::release(&mut app));
        extrusion::flush_for_test(&mut app, &ctx).unwrap();
        assert!(!extrusion::active(&app));
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
        settle_released_extrusion(&mut h);
        let form = h.app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.cage, before.cage, "{:?}", h.app.status);
        let first = before.cage.points.len();
        assert_eq!(form.extruded_edges(), &[[first, first + 1]], "{:?}", h.app.status);
        assert_eq!(h.app.forms_prototype.elements, BTreeSet::from([Element::Edge([first, first + 1])]));
        assert!(form.selectable_control(first));
        assert_eq!(form.logical.faces.len(), before.hierarchy.unwrap().leaves.len() + 1);
        assert_eq!(form.inspect()["undo"], 3);
        let h_form = form.hierarchy.as_ref().unwrap();
        let wall = h_form.validated_trim_wall().unwrap().unwrap();
        let a = wall.attachment().edge[0];
        live_cad::install_for_test(&mut h.app);
        let exact = live_cad::selection_geometry(&h.app).unwrap();
        let solidcraft_engine::doc::forms::SelectionGuides::Subdivision(curves) = &exact.guides else { panic!("missing exact trim wall guides") };
        let cap = curves.iter().find(|curve| curve.edge == [first, first + 1]).unwrap();
        let side = curves.iter().find(|curve| curve.edge == [a, first]).unwrap();
        let (range, cap) = cap.rational_pieces().unwrap().into_iter().find(|(r, _)| r[0] <= 0.5 && r[1] >= 0.5).unwrap();
        let domain = [cap.knots[cap.degree], cap.knots[cap.ctrl.len()]];
        let t = (0.5 - range[0]) / (range[1] - range[0]);
        let (_, tangent) = cap.eval(domain[0] * (1.0 - t) + domain[1] * t).unwrap();
        let tangent = Vec3::new(tangent[0], tangent[1], tangent[2]).normalized().unwrap();
        let sweep = (side.at(1.0).unwrap() - side.at(0.0).unwrap()).normalized().unwrap();
        h.app.forms_prototype.orientation = 2;
        h.app.forms_prototype.gizmo_drag = None;
        let directions = basis(&h.app).unwrap();
        assert!(directions[0].dot(tangent) > 0.999999);
        assert!(directions[2].dot(tangent).abs() < 1e-6);
        assert!(directions[2].dot(sweep).abs() < 1e-6);
        assert!(directions[0].cross(directions[1]).dot(directions[2]) > 0.999999);
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(h.app.session.forms_prototype.as_ref().unwrap().hierarchy.as_ref().is_some_and(|h| !h.has_trim_wall()));
    }

    #[test]
    fn exact_selection_axes_follow_placement_for_faces_edges_and_points() {
        for refined in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[12,9,1],"segments":[3,3,1],"levels":0})).unwrap();
            if refined {
                app.session.execute("form.prototype.refine", &json!({"faces":[4],"counts":[2,1]})).unwrap();
            }
            let face = if refined { 9 } else { 4 };
            let f = app.session.forms_prototype.as_ref().unwrap();
            let [a, b] = [f.logical.faces[face][0], f.logical.faces[face][1]];
            let elements = [Element::Face(face), Element::Edge([a.min(b), a.max(b)]), Element::Vertex(a)];
            app.forms_prototype.orientation = 2;
            live_cad::install_for_test(&mut app);
            let mut original = Vec::new();
            for element in elements {
                app.forms_prototype.elements = BTreeSet::from([element]);
                let dirs = basis(&app).unwrap();
                assert!(dirs[2].dist(Vec3::Z) < 1e-8);
                assert!(dirs[0].cross(dirs[1]).dist(dirs[2]) < 1e-8);
                original.push(dirs);
            }
            fit(&mut app);
            app.forms_prototype.pick = Pick::Vertex;
            app.forms_prototype.axis = Axis::X;
            let projection = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
            let at = projection.to_screen(app.session.forms_prototype.as_ref().unwrap().surface.vertices[a]).unwrap();
            begin_drag(&mut app, &projection, at, egui::Modifiers::NONE);
            let drag = app.forms_prototype.drag.as_ref().unwrap();
            assert_eq!(drag.basis, original[2]);
            assert_eq!(drag.constraint_axes, original[2]);
            app.cam.roll = 0.74;
            assert_eq!(app.forms_prototype.drag.as_ref().unwrap().basis, original[2]);
            app.forms_prototype.drag = None;
            selection::cancel_press(&mut app);
            let axis = Vec3::new(0.6, 0.8, 0.0);
            let matrix = solidcraft_engine::doc::rigid(Vec3::new(6.0, 2.0, -3.0), Vec3::ZERO, axis, std::f64::consts::PI);
            let placement = solidcraft_engine::doc::forms::Placement::from_matrix(&matrix).unwrap();
            app.session.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
            assert_eq!(basis(&app), Err("forms.selection_frame.current_cad"));
            app.cam.roll = 0.74;
            live_cad::tick(&mut app, &egui::Context::default());
            assert!(live_cad::selection_geometry(&app).is_some());
            for (element, before) in elements.into_iter().zip(original) {
                app.forms_prototype.elements = BTreeSet::from([element]);
                let dirs = basis(&app).unwrap();
                for (actual, local) in dirs.into_iter().zip(before) {
                    assert!(actual.dist(placement.vector(local)) < 1e-8);
                }
                assert!(dirs[2].dist(-Vec3::Z) < 1e-8);
                assert!(dirs[0].cross(dirs[1]).dist(dirs[2]) < 1e-8);
            }
            app.forms_prototype.elements.extend(elements);
            app.forms_prototype.selection.active = None;
            assert_eq!(basis(&app), Err("forms.selection_frame.single_element"));
        }
    }

    #[test]
    fn mixed_selection_axes_follow_the_last_direct_pick_and_forget_a_removed_member() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        fit(&mut app);
        live_cad::install_for_test(&mut app);
        app.forms_prototype.orientation = 2;
        app.forms_prototype.pick = Pick::Vertex;
        let face = Element::Face(0);
        // Point 4 is the rounded corner with collinear fitted derivatives.
        // This gesture tests a point with its own qualified selection frame.
        let vertex_id = 1;
        let vertex = Element::Vertex(vertex_id);
        let exact = live_cad::selection_geometry(&app).unwrap();
        let face_axes = exact.frame(face).unwrap();
        let vertex_axes = exact.frame(vertex).unwrap();
        assert!(face_axes[0].dist(vertex_axes[0]) > 0.1, "the fixture must discriminate the two frames");
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let screen = proj.to_screen(exact.points.as_ref().unwrap()[&vertex_id]).unwrap();
        assert_eq!(surface::nearest(&app, &proj, screen), Some(vertex));

        app.forms_prototype.elements = BTreeSet::from([face]);
        assert_eq!(basis(&app), Ok(face_axes));
        begin_drag(&mut app, &proj, screen, egui::Modifiers::COMMAND);
        assert_eq!(app.forms_prototype.elements, BTreeSet::from([face, vertex]));
        assert_eq!(selection::active(&app), Some(vertex));
        assert_eq!(app.forms_prototype.drag.as_ref().unwrap().basis, vertex_axes);
        assert_eq!(basis(&app), Ok(vertex_axes));
        let click = app.forms_prototype.drag.take().unwrap();
        selection::released(&mut app, click.toggle);

        begin_drag(&mut app, &proj, screen, egui::Modifiers::COMMAND);
        let click = app.forms_prototype.drag.take().unwrap();
        selection::released(&mut app, click.toggle);
        assert_eq!(app.forms_prototype.elements, BTreeSet::from([face]));
        assert_eq!(selection::active(&app), None);
        assert_eq!(basis(&app), Ok(face_axes));
        select_all(&mut app);
        assert!(app.forms_prototype.elements.len() > 1);
        assert_eq!(basis(&app), Err("forms.selection_frame.single_element"));
    }

    #[test]
    fn missing_selection_frame_keeps_click_and_refuses_to_author_a_drag() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        fit(&mut app);
        app.forms_prototype.pick = Pick::Vertex;
        app.forms_prototype.orientation = 2;
        let projection = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let f = app.session.forms_prototype.as_ref().unwrap();
        let id = f.cage.points.iter().position(|&p| p == Vec3::ZERO).unwrap();
        let point = projection.to_screen(f.surface.vertices[id]).unwrap();
        let source = f.definition();
        begin_drag(&mut app, &projection, point, egui::Modifiers::NONE);
        assert!(app.forms_prototype.drag.is_none());
        pull(&mut app, point + egui::vec2(30.0, 10.0));
        selection::release_blocked_drag(&mut app);
        assert_eq!(app.forms_prototype.elements, BTreeSet::from([Element::Vertex(id)]));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
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
    fn cancelling_a_transform_only_undoes_its_current_authored_edit() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        app.session.execute("form.prototype.symmetry", &json!({"axis":0})).unwrap();
        fit(&mut app);
        app.forms_prototype.pick = Pick::Vertex;
        let proj = crate::viewport::projection(&app, Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let form = app.session.forms_prototype.as_ref().unwrap();
        let vertex = form.cage.points.iter().position(|&point| point == Vec3::ZERO).unwrap();
        let point = form.surface.vertices[vertex];
        let before = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        assert!(undo > 0);
        let start = proj.to_screen(point).unwrap();
        let end_x = proj.to_screen(point + Vec3::X * 3.0).unwrap();
        let end_y = proj.to_screen(point + Vec3::Y * 3.0).unwrap();
        let ctx = egui::Context::default();
        let id = egui::Id::unique("cancel_form_transform");

        app.forms_prototype.axis = Axis::X;
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        assert_eq!(app.forms_prototype.drag.as_ref().unwrap().indices, [vertex]);
        pull(&mut app, end_x);
        let drag = app.forms_prototype.drag.as_ref().unwrap();
        assert!(drag.moved && !drag.authored, "the symmetry plane constrains this sample to a no-op");
        cancel_transform_gesture(&mut app, &ctx, id);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);

        // A no-op sample must leave the first checkpoint available to the
        // first effective sample, such as free motion along the symmetry plane.
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        pull(&mut app, end_x);
        app.forms_prototype.drag.as_mut().unwrap().axis = Axis::Y;
        pull(&mut app, end_y);
        assert!(app.forms_prototype.drag.as_ref().unwrap().authored);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo + 1);
        cancel_transform_gesture(&mut app, &ctx, id);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);

        app.forms_prototype.axis = Axis::Y;
        begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
        pull(&mut app, end_y);
        assert!(app.forms_prototype.drag.as_ref().unwrap().authored);
        let unrelated = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::Z;
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[unrelated.x,unrelated.y,unrelated.z]]})).unwrap();
        let latest = app.session.forms_prototype.as_ref().unwrap().definition();
        pull(&mut app, end_y + egui::vec2(60.0, -40.0));
        cancel_transform_gesture(&mut app, &ctx, id);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), latest);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo + 2);
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
        let ctx = egui::Context::default();
        let primary = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
        app.forms_prototype.pick = Pick::Vertex;
        assert!(shortcut(&mut app, &ctx, egui::Key::A, primary));
        assert_eq!(app.forms_prototype.elements.len(), 9);
        assert!(app.forms_prototype.elements.iter().all(|e| matches!(e, Element::Vertex(_))));
        assert!(shortcut(&mut app, &ctx, egui::Key::Delete, egui::Modifiers::NONE));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage.faces.len(), 4);
        app.forms_prototype.pick = Pick::Edge;
        shortcut(&mut app, &ctx, egui::Key::A, primary);
        assert_eq!(app.forms_prototype.elements.len(), 12);
        app.forms_prototype.elements = [Element::Face(0)].into_iter().collect();
        selection::sync(&mut app);
        assert!(shortcut(&mut app, &ctx, egui::Key::Delete, egui::Modifiers::NONE));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().cage.faces.len(), 3);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 1);
        assert!(app.session.doc.features.is_empty());
        assert!(app.forms_prototype.elements.is_empty());
        app.forms_prototype.pick = Pick::Face;
        shortcut(&mut app, &ctx, egui::Key::A, primary);
        assert_eq!(app.forms_prototype.elements.len(), 3);
        assert!(!shortcut(&mut app, &ctx, egui::Key::F, egui::Modifiers::NONE));
        tool_panel::start(&mut app, tool_panel::Mode::Thicken);
        shortcut(&mut app, &ctx, egui::Key::Delete, egui::Modifiers::NONE);
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
