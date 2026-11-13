//! Shared source-curve preparation and picking for Forms and Insert Point.
use super::*;
use solidcore_ops::forms_view_queries::{self, BoundsIndex, MAX_CURVES, Projection};

type Curve = solidcraft_engine::doc::forms::EdgeCurve;
pub(super) const HIT_PIXELS: f32 = 9.0;
const MAX_CANDIDATES: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Key {
    document: String,
    authoring: u64,
    draft: solidcraft_engine::recovery::FormRevision,
    preview: (u64, bool),
    context: u64,
    tolerance: u64,
}
pub(super) fn source(app: &SolveApp) -> Option<Key> {
    let form = app.session.forms_prototype.as_ref()?;
    // Profile guides and derived wall rims have separate
    // provenance. Keep their existing picker until exact mappings exist.
    if form.profile.is_some() || form.hierarchy.as_ref().is_some_and(|h| h.has_trim_wall()) {
        return None;
    }
    Some(Key {
        document: app.session.document_id.clone(),
        authoring: app.session.authoring_revision(),
        draft: solidcraft_engine::recovery::form_revision(&app.session)?,
        preview: (form.revision, form.has_tool_preview()),
        context: context::key(app),
        tolerance: controls::tolerance(app)?.to_bits(),
    })
}

struct Prepared {
    curves: Vec<Curve>,
    edges: std::collections::BTreeMap<[usize; 2], usize>,
    index: BoundsIndex,
}
impl Prepared {
    fn new(curves: Vec<Curve>) -> Result<Self, String> {
        if curves.len() > MAX_CURVES {
            return Err("Form curve picking budget exceeded".into());
        }
        let mut edges = std::collections::BTreeMap::new();
        for (index, curve) in curves.iter().enumerate() {
            if edges.insert(curve.edge, index).is_some() {
                return Err("Form curve bounds or identities are invalid".into());
            }
        }
        let index = BoundsIndex::new(curves.iter().map(|curve| {
            let bounds = curve.bounds();
            [bounds.min, bounds.max]
        }))
        .ok_or("Form curve bounds or identities are invalid")?;
        Ok(Self { curves, edges, index })
    }
}
struct Pending {
    key: Key,
    done: std::sync::mpsc::Receiver<Result<Prepared, String>>,
    #[cfg(test)]
    injected: Option<std::sync::mpsc::Receiver<Result<Vec<Curve>, String>>>,
}
impl Pending {
    fn result(&self) -> Option<Result<Prepared, String>> {
        #[cfg(test)]
        if let Some(injected) = &self.injected {
            return match injected.try_recv() {
                Ok(result) => Some(result.and_then(Prepared::new)),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err("Form curve preparation stopped".into())),
            };
        }
        match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Form curve preparation stopped".into())),
        }
    }
}
#[derive(Default)]
pub(super) struct State {
    pending: Option<Pending>,
    ready: Option<(Key, Result<Prepared, String>)>,
}
fn prepared(app: &SolveApp) -> Option<Result<&Prepared, &str>> {
    let key = source(app)?;
    app.forms_prototype.curves.ready.as_ref().filter(|(given, _)| *given == key).map(|(_, result)| result.as_ref().map_err(String::as_str))
}
pub(super) fn current(app: &SolveApp) -> bool {
    prepared(app).is_some()
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    let ready = prepared(app);
    let edges = ready.as_ref().and_then(|result| result.as_ref().ok()).map_or(0, |prepared| prepared.curves.len());
    json!({"pending":app.forms_prototype.curves.pending.is_some(),
        "ready":ready.as_ref().is_some_and(|result|result.is_ok()),
        "error":ready.as_ref().and_then(|result|result.as_ref().err()),
        "edges":edges})
}
pub(super) fn show_status(app: &SolveApp, ui: &mut egui::Ui) {
    if exact_picking(app) && !current(app) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(app.tr("forms.prototype.previewing"));
        });
    }
    if let Some(Err(error)) = ready(app) {
        ui.add(egui::Label::new(egui::RichText::new(error).color(Tokens::get().warning)).wrap());
    }
}
pub(super) fn ready(app: &SolveApp) -> Option<Result<&[Curve], &str>> {
    prepared(app).map(|result| result.map(|prepared| prepared.curves.as_slice()))
}
pub(super) fn get(app: &SolveApp, edge: [usize; 2]) -> Option<&Curve> {
    let prepared = prepared(app)?.ok()?;
    let key = [edge[0].min(edge[1]), edge[0].max(edge[1])];
    prepared.edges.get(&key).and_then(|&index| prepared.curves.get(index))
}
fn compute(session: &solidcraft_engine::Session, tolerance: f64) -> Result<Prepared, String> {
    Prepared::new(solidcraft_engine::forms_prototype::insertion_curves(session, tolerance, &[]).map_err(|error| error.to_string())?)
}
fn poll(app: &mut SolveApp) {
    let state = &mut app.forms_prototype.curves;
    let Some(pending) = &state.pending else { return };
    let Some(result) = pending.result() else { return };
    let key = pending.key.clone();
    state.pending = None;
    if source(app).as_ref() == Some(&key) {
        app.forms_prototype.curves.ready = Some((key, result));
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn start(app: &mut SolveApp, ctx: &egui::Context) {
    if current(app) || app.forms_prototype.curves.pending.is_some() {
        return;
    }
    let Some(key) = source(app) else { return };
    let session = app.session.scratch();
    let tolerance = f64::from_bits(key.tolerance);
    let (tx, done) = std::sync::mpsc::channel();
    let wake = ctx.clone();
    match std::thread::Builder::new().name("form-source-curves".into()).spawn(move || {
        let _ = tx.send(compute(&session, tolerance));
        wake.request_repaint();
    }) {
        Ok(_) => {
            app.forms_prototype.curves.pending = Some(Pending {
                key,
                done,
                #[cfg(test)]
                injected: None,
            })
        }
        Err(error) => app.forms_prototype.curves.ready = Some((key, Err(error.to_string()))),
    }
}
pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    poll(app);
    #[cfg(not(target_arch = "wasm32"))]
    if app.forms_prototype.drag.is_none()
        && app.forms_prototype.gizmo_drag.is_none()
        && !extrusion::active(app)
        && app.forms_prototype.pending.is_none()
    {
        start(app, ctx);
    }
    if app.forms_prototype.curves.pending.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
pub(super) fn prepare_explicit(app: &mut SolveApp, ctx: &egui::Context) {
    poll(app);
    #[cfg(not(target_arch = "wasm32"))]
    start(app, ctx);
    #[cfg(target_arch = "wasm32")]
    if !current(app)
        && let Some(key) = source(app)
    {
        let result = compute(&app.session.scratch(), f64::from_bits(key.tolerance));
        app.forms_prototype.curves.ready = Some((key, result));
        ctx.request_repaint();
    }
}
/// Native source picking waits for its shared worker. Browsers may reuse a cache
/// explicitly prepared by Insert Point until a browser worker host is available.
pub(super) fn exact_picking(app: &SolveApp) -> bool {
    source(app).is_some() && (cfg!(not(target_arch = "wasm32")) || current(app)) && live_cad::mesh(app).is_some()
}
pub(super) fn projection(proj: &Proj) -> Projection {
    Projection { matrix: proj.vp.0, origin: [proj.rect.left(), proj.rect.top()], size: [proj.rect.width(), proj.rect.height()] }
}
fn projections(curve: &Curve, projection: Projection, position: Pos2) -> impl Iterator<Item = (f32, f64, Vec3)> {
    forms_view_queries::curve_hits(projection, [position.x, position.y], HIT_PIXELS, |fraction| curve.at(fraction).ok())
}
#[cfg(test)]
pub(super) fn project(curve: &Curve, proj: &Proj, position: Pos2) -> Option<(f64, Vec3)> {
    projections(curve, projection(proj), position).next().map(|(_, fraction, point)| (fraction, point))
}
pub(super) fn nearest(app: &SolveApp, proj: &Proj, position: Pos2) -> Option<([usize; 2], f64, Vec3)> {
    let prepared = prepared(app)?.ok()?;
    let projected_view = projection(proj);
    // A crowded viewport refuses the pick if it exceeds the candidate budget.
    let candidates = prepared.index.candidates(projected_view, [position.x, position.y], HIT_PIXELS, MAX_CANDIDATES)?;
    let mut best: Option<(f32, [usize; 2], f64, Vec3)> = None;
    for index in candidates {
        let curve = prepared.curves.get(index)?;
        let hit = projections(curve, projected_view, position).find(|(_, _, point)| surface::visible(app, proj, *point));
        if let Some((distance, fraction, point)) = hit
            && best.as_ref().is_none_or(|old| distance < old.0 || distance == old.0 && curve.edge < old.1)
        {
            best = Some((distance, curve.edge, fraction, point));
        }
    }
    best.map(|(_, edge, fraction, point)| (edge, fraction, point))
}

#[cfg(test)]
pub(super) fn install_for_test(app: &mut SolveApp, result: Result<Vec<Curve>, String>) {
    let key = source(app).unwrap();
    app.forms_prototype.curves = State { pending: None, ready: Some((key, result.and_then(Prepared::new))) };
}
#[cfg(test)]
pub(super) fn clear_for_test(app: &mut SolveApp) {
    app.forms_prototype.curves = State::default();
}
#[cfg(test)]
pub(super) fn pending_for_test(app: &mut SolveApp) -> std::sync::mpsc::Sender<Result<Vec<Curve>, String>> {
    let key = source(app).unwrap();
    let (sender, injected) = std::sync::mpsc::channel();
    let (_, done) = std::sync::mpsc::channel();
    app.forms_prototype.curves = State { pending: Some(Pending { key, done, injected: Some(injected) }), ready: None };
    sender
}
#[cfg(test)]
pub(super) fn poll_for_test(app: &mut SolveApp) {
    poll(app);
}
