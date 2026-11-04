//! Shared source-curve preparation and picking for Forms and Insert Point.
use super::*;
use solidcraft_engine::geom::Aabb3;

type Curve = solidcraft_engine::doc::forms::EdgeCurve;
const HIT_PIXELS: f32 = 9.0;
const MAX_CURVES: usize = 65_536;
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

struct Node {
    bounds: Aabb3,
    children: Option<Box<[Node; 2]>>,
    indices: Vec<usize>,
}
impl Node {
    fn build(curves: &[Curve], indices: &mut [usize]) -> Self {
        let mut bounds = Aabb3 {
            min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        };
        for &index in indices.iter() {
            let next = curves[index].bounds();
            bounds.min = Vec3::new(bounds.min.x.min(next.min.x), bounds.min.y.min(next.min.y), bounds.min.z.min(next.min.z));
            bounds.max = Vec3::new(bounds.max.x.max(next.max.x), bounds.max.y.max(next.max.y), bounds.max.z.max(next.max.z));
        }
        if indices.len() <= 8 {
            return Self { bounds, children: None, indices: indices.to_vec() };
        }
        let extents = [bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y, bounds.max.z - bounds.min.z];
        let mut axis = 0;
        for next in 1..3 {
            if extents[next] > extents[axis] {
                axis = next;
            }
        }
        let centre = |index: usize| {
            let bounds = curves[index].bounds();
            [bounds.min.x * 0.5 + bounds.max.x * 0.5, bounds.min.y * 0.5 + bounds.max.y * 0.5, bounds.min.z * 0.5 + bounds.max.z * 0.5][axis]
        };
        indices.sort_unstable_by(|&a, &b| centre(a).total_cmp(&centre(b)).then(a.cmp(&b)));
        let middle = indices.len() / 2;
        let (left, right) = indices.split_at_mut(middle);
        Self { bounds, children: Some(Box::new([Self::build(curves, left), Self::build(curves, right)])), indices: Vec::new() }
    }
    fn candidates(&self, curves: &[Curve], proj: &Proj, position: Pos2, output: &mut Vec<usize>) {
        if output.len() > MAX_CANDIDATES || !bounds_hit(&self.bounds, proj, position) {
            return;
        }
        if let Some(children) = &self.children {
            for child in children.iter() {
                child.candidates(curves, proj, position, output);
            }
        } else {
            output.extend(
                self.indices.iter().copied().filter(|&index| curves.get(index).is_some_and(|curve| bounds_hit(&curve.bounds(), proj, position))),
            );
        }
    }
}
struct Prepared {
    curves: Vec<Curve>,
    edges: std::collections::BTreeMap<[usize; 2], usize>,
    index: Option<Node>,
}
impl Prepared {
    fn new(curves: Vec<Curve>) -> Result<Self, String> {
        if curves.len() > MAX_CURVES {
            return Err("Form curve picking budget exceeded".into());
        }
        let mut edges = std::collections::BTreeMap::new();
        for (index, curve) in curves.iter().enumerate() {
            let bounds = curve.bounds();
            if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() || edges.insert(curve.edge, index).is_some() {
                return Err("Form curve bounds or identities are invalid".into());
            }
        }
        let mut indices: Vec<_> = (0..curves.len()).collect();
        let index = (!indices.is_empty()).then(|| Node::build(&curves, &mut indices));
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
fn bounds_hit(bounds: &Aabb3, proj: &Proj, position: Pos2) -> bool {
    let mut screen = egui::Rect::NOTHING;
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                let Some(point) = proj.to_screen(Vec3::new(x, y, z)) else { return true };
                // A box crossing the eye plane or overflowing projection cannot
                // be rejected safely. Keep it as a candidate instead.
                if !point.x.is_finite() || !point.y.is_finite() {
                    return true;
                }
                screen.extend_with(point);
            }
        }
    }
    screen.expand(HIT_PIXELS).contains(position)
}
fn projections(curve: &Curve, proj: &Proj, position: Pos2) -> Vec<(f32, f64, Vec3)> {
    const SAMPLES: usize = 64;
    let evaluate = |t| {
        let point = curve.at(t).ok()?;
        let screen = proj.to_screen(point)?;
        let distance = screen.distance_sq(position);
        distance.is_finite().then_some((distance, t, point))
    };
    let sampled: Vec<_> = (0..=SAMPLES).map(|i| evaluate(i as f64 / SAMPLES as f64)).collect();
    let mut candidates = Vec::new();
    for (i, value) in sampled.iter().enumerate() {
        let Some(value) = value else { continue };
        if i == 0 || i == SAMPLES {
            candidates.push(*value);
        }
        let distance = value.0;
        if i > 0 && sampled[i - 1].is_some_and(|other| other.0 <= distance) || i < SAMPLES && sampled[i + 1].is_some_and(|other| other.0 < distance) {
            continue;
        }
        candidates.push(*value);
        let mut low = i.saturating_sub(1) as f64 / SAMPLES as f64;
        let mut high = (i + 1).min(SAMPLES) as f64 / SAMPLES as f64;
        for _ in 0..24 {
            let a = low + (high - low) / 3.0;
            let b = high - (high - low) / 3.0;
            let (Some(va), Some(vb)) = (evaluate(a), evaluate(b)) else { break };
            if va.0 <= vb.0 {
                high = b;
            } else {
                low = a;
            }
        }
        if let Some(value) = evaluate((low + high) * 0.5) {
            candidates.push(value);
        }
    }
    candidates.retain(|value| value.0 <= HIT_PIXELS * HIT_PIXELS);
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    candidates
}
pub(super) fn project(curve: &Curve, proj: &Proj, position: Pos2) -> Option<(f64, Vec3)> {
    projections(curve, proj, position).first().map(|value| (value.1, value.2))
}
pub(super) fn nearest(app: &SolveApp, proj: &Proj, position: Pos2) -> Option<([usize; 2], f64, Vec3)> {
    let prepared = prepared(app)?.ok()?;
    let mut candidates = Vec::new();
    prepared.index.as_ref()?.candidates(&prepared.curves, proj, position, &mut candidates);
    // Never pick from a truncated prefix: in exceptionally crowded geometry
    // the closest visible edge has not been qualified within this query budget.
    if candidates.len() > MAX_CANDIDATES {
        return None;
    }
    let mut best: Option<(f32, [usize; 2], f64, Vec3)> = None;
    for index in candidates {
        let curve = prepared.curves.get(index)?;
        if !bounds_hit(&curve.bounds(), proj, position) {
            continue;
        }
        let Some((fraction, point)) = project(curve, proj, position) else { continue };
        let hit = if surface::visible(app, proj, point) {
            proj.to_screen(point).map(|screen| (screen.distance_sq(position), fraction, point))
        } else {
            projections(curve, proj, position).into_iter().find(|(_, _, point)| surface::visible(app, proj, *point))
        };
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
