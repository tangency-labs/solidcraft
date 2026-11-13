//! Topological ranges and loop selection operate on editable elements, not triangles.
use super::*;
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
pub(super) struct State {
    pub(super) active: Option<Element>,
    anchors: [Option<Element>; 3],
    pending: Option<(Element, bool)>,
    blocked_drag: Option<Option<Element>>,
    topology: u64,
}
fn kind(element: Element) -> usize {
    match element {
        Element::Vertex(_) => 0,
        Element::Edge(_) => 1,
        Element::Face(_) | Element::Surface => 2,
    }
}
type Graph = BTreeMap<Element, BTreeSet<Element>>;
fn connect(graph: &mut Graph, a: Element, b: Element) {
    if a != b {
        graph.entry(a).or_default().insert(b);
        graph.entry(b).or_default().insert(a);
    }
}
fn edge(a: usize, b: usize) -> Element {
    Element::Edge([a.min(b), a.max(b)])
}
fn graph(faces: &[Vec<usize>], element: Element) -> Option<Graph> {
    let mut uses: BTreeMap<Element, Vec<usize>> = BTreeMap::new();
    for (id, face) in faces.iter().enumerate() {
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            uses.entry(edge(a, b)).or_default().push(id);
        }
    }
    let mut out = Graph::new();
    match element {
        Element::Surface => {
            out.entry(Element::Surface).or_default();
        }
        Element::Vertex(_) => {
            for e in uses.keys() {
                if let Element::Edge([a, b]) = *e {
                    connect(&mut out, Element::Vertex(a), Element::Vertex(b));
                }
            }
        }
        Element::Edge(_) => {
            let mut incident: BTreeMap<usize, Vec<Element>> = BTreeMap::new();
            for &e in uses.keys() {
                out.entry(e).or_default();
                if let Element::Edge([a, b]) = e {
                    incident.entry(a).or_default().push(e);
                    incident.entry(b).or_default().push(e);
                }
            }
            for neighbors in incident.values() {
                if neighbors.len() > 64 {
                    return None;
                }
                for (i, &a) in neighbors.iter().enumerate() {
                    for &b in neighbors.iter().skip(i + 1) {
                        connect(&mut out, a, b);
                    }
                }
            }
        }
        Element::Face(_) => {
            for (id, _) in faces.iter().enumerate().filter(|(_, f)| !f.is_empty()) {
                out.entry(Element::Face(id)).or_default();
            }
            for ids in uses.values().filter(|ids| ids.len() == 2) {
                if let (Some(&a), Some(&b)) = (ids.first(), ids.get(1)) {
                    connect(&mut out, Element::Face(a), Element::Face(b));
                }
            }
        }
    }
    Some(out)
}
fn distances(graph: &Graph, start: Element) -> BTreeMap<Element, usize> {
    let mut distance = BTreeMap::new();
    if !graph.contains_key(&start) {
        return distance;
    }
    distance.insert(start, 0);
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        let depth = distance.get(&node).copied().unwrap_or(0) + 1;
        for &next in graph.get(&node).into_iter().flatten() {
            if let std::collections::btree_map::Entry::Vacant(entry) = distance.entry(next) {
                entry.insert(depth);
                queue.push_back(next);
            }
        }
    }
    distance
}
fn range(faces: &[Vec<usize>], first: Element, last: Element) -> Option<BTreeSet<Element>> {
    if kind(first) != kind(last) {
        return None;
    }
    let graph = graph(faces, first)?;
    let a = distances(&graph, first);
    let b = distances(&graph, last);
    let total = *a.get(&last)?;
    Some(a.into_iter().filter_map(|(node, d)| b.get(&node).is_some_and(|r| d + r == total).then_some(node)).collect())
}
fn edge_loop(faces: &[Vec<usize>], seed: Element) -> BTreeSet<Element> {
    let mut uses: BTreeMap<Element, BTreeSet<usize>> = BTreeMap::new();
    let mut incident: BTreeMap<usize, BTreeSet<Element>> = BTreeMap::new();
    for (id, face) in faces.iter().enumerate() {
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let e = edge(a, b);
            uses.entry(e).or_default().insert(id);
            incident.entry(a).or_default().insert(e);
            incident.entry(b).or_default().insert(e);
        }
    }
    if !uses.contains_key(&seed) {
        return BTreeSet::new();
    }
    let mut selected = BTreeSet::new();
    let mut pending = vec![seed];
    while let Some(e) = pending.pop() {
        if !selected.insert(e) {
            continue;
        }
        if let Element::Edge(vertices) = e {
            for v in vertices {
                let candidates: Vec<_> = incident
                    .get(&v)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|other| *other != e && uses.get(&e).zip(uses.get(other)).is_some_and(|(a, b)| a.is_disjoint(b)))
                    .collect();
                if candidates.len() == 1
                    && let Some(&next) = candidates.first()
                {
                    pending.push(next);
                }
            }
        }
    }
    selected
}
pub(super) fn pressed(app: &mut SolveApp, hit: Element, shift: bool) {
    if let Some(form) = &app.session.forms_prototype {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        form.logical.faces.hash(&mut hash);
        let key = hash.finish();
        if app.forms_prototype.selection.topology != key {
            app.forms_prototype.selection = State { topology: key, ..Default::default() };
        }
    }
    activate(app, hit);
    app.forms_prototype.selection.pending = Some((hit, shift));
    app.forms_prototype.selection.blocked_drag = None;
    if !shift {
        app.forms_prototype.selection.anchors[kind(hit)] = Some(hit);
    }
}
pub(super) fn activate(app: &mut SolveApp, hit: Element) {
    if app.forms_prototype.elements.contains(&hit) {
        app.forms_prototype.selection.active = Some(hit);
    }
}

pub(super) fn active(app: &SolveApp) -> Option<Element> {
    app.forms_prototype.selection.active.filter(|element| app.forms_prototype.elements.contains(element))
}

pub(super) fn blocked_drag(app: &mut SolveApp, toggle: Option<Element>) {
    app.forms_prototype.selection.blocked_drag = Some(toggle);
}
pub(super) fn release_blocked_drag(app: &mut SolveApp) {
    if let Some(toggle) = app.forms_prototype.selection.blocked_drag.take() {
        released(app, toggle);
    }
}
pub(super) fn sync(app: &mut SolveApp) {
    let Some(form) = &app.session.forms_prototype else {
        app.forms_prototype.selection.active = None;
        return;
    };
    if let Some(logical) = live_cad::selection_geometry(app).and_then(|exact| exact.logical.as_ref().ok()).cloned() {
        app.forms_prototype.elements.retain(|element| logical.active.contains(element));
    } else {
        let vertices: BTreeSet<_> = form.logical.faces.iter().flatten().copied().collect();
        let mut edges: BTreeSet<_> = form.surface.boundaries.iter().map(|(e, _)| edge(e[0], e[1])).collect();
        edges.extend(logical_edges(&form.logical.faces));
        app.forms_prototype.elements.retain(|e| match e {
            Element::Surface => form.profile.is_some(),
            Element::Face(id) => form.logical.faces.get(*id).is_some_and(|f| !f.is_empty()),
            Element::Vertex(id) => vertices.contains(id),
            Element::Edge([a, b]) => edges.contains(&edge(*a, *b)),
        });
    }
    app.forms_prototype.selected = app.forms_prototype.elements.iter().flat_map(|e| e.vertices(&form.logical)).collect();
    app.forms_prototype.selection.active = active(app);
}
pub(super) fn released(app: &mut SolveApp, toggle: Option<Element>) {
    let before = app.forms_prototype.elements.clone();
    let pending = app.forms_prototype.selection.pending.take();
    let mut extended = false;
    if let Some((hit, true)) = pending {
        let anchor = app.forms_prototype.selection.anchors[kind(hit)];
        if let Some(anchor) = anchor.filter(|a| *a != hit)
            && let Some(form) = &app.session.forms_prototype
            && let Some(elements) = range(&form.logical.faces, anchor, hit)
        {
            app.forms_prototype.elements.extend(elements);
            extended = true;
        } else {
            app.forms_prototype.selection.anchors[kind(hit)] = Some(hit);
        }
    }
    if !extended && let Some(e) = toggle {
        app.forms_prototype.elements.remove(&e);
    }
    if before != app.forms_prototype.elements {
        app.forms_prototype.pivot = None;
    }
    sync(app);
    if let Some((hit, _)) = pending {
        activate(app, hit);
    }
}
pub(super) fn cancel_press(app: &mut SolveApp) {
    app.forms_prototype.selection.pending = None;
    app.forms_prototype.selection.blocked_drag = None;
}
pub(super) fn expand(app: &mut SolveApp, connected: bool) {
    let Some(form) = &app.session.forms_prototype else { return };
    let mut selected = app.forms_prototype.elements.clone();
    if connected {
        for category in 0..3 {
            let seeds: Vec<_> = app.forms_prototype.elements.iter().copied().filter(|e| kind(*e) == category).collect();
            let Some(&first) = seeds.first() else { continue };
            let Some(graph) = graph(&form.logical.faces, first) else { continue };
            let mut covered = BTreeSet::new();
            for seed in seeds {
                if !covered.contains(&seed) {
                    covered.extend(distances(&graph, seed).into_keys());
                }
            }
            selected.extend(covered);
        }
    } else {
        let mut covered = BTreeSet::new();
        for &e in &app.forms_prototype.elements {
            if matches!(e, Element::Edge(_)) && !covered.contains(&e) {
                covered.extend(edge_loop(&form.logical.faces, e));
            }
        }
        selected.extend(covered);
    }
    app.forms_prototype.elements = selected;
    app.forms_prototype.pivot = None;
    sync(app);
}

// This gesture owns UI selection only. Geometry classification runs on a
// bounded native worker over Query's immutable exact/display snapshot.
const MAX_REGION_POINTS: usize = 2048;
#[cfg(test)]
const MAX_REGION_USES: usize = 262_144;
const REGION_DRAG_PIXELS: f32 = 4.0;
type RegionResult = Result<BTreeSet<Element>, String>;

#[derive(Clone, PartialEq)]
struct RegionKey {
    document: u64,
    authoring: u64,
    display: u64,
    source: (u64, u64),
    preview: bool,
    context: u64,
    context_version: u64,
    tolerance: u64,
    operation: usize,
    combination: (bool, bool),
    partition: Option<bool>,
    pick: Pick,
    lasso: bool,
    greedy: bool,
}
impl RegionKey {
    fn current(app: &SolveApp) -> Option<Self> {
        let form = app.session.forms_prototype.as_ref()?;
        Some(Self {
            document: app.session.uid,
            authoring: app.session.authoring_revision(),
            display: app.session.revision,
            source: form.source_revision(),
            preview: form.has_tool_preview(),
            context: context::key(app),
            context_version: app.forms_prototype.context.version,
            tolerance: controls::tolerance(app)?.to_bits(),
            operation: controls::operation(app),
            combination: (app.forms_prototype.show_combination, combine_preview::ready(app).is_some()),
            partition: form.profile.as_ref().map(|profile| profile.partition),
            pick: app.forms_prototype.pick,
            lasso: app.ui.lasso_select,
            greedy: app.preferences.forms_greedy_selection,
        })
    }
}
#[derive(Clone, Copy)]
enum RegionMode {
    Replace,
    Add,
    Toggle,
}
#[derive(Clone)]
pub(super) struct RegionDrag {
    start: Pos2,
    shape: crate::selection::BoxSel,
    projection: Proj,
    key: RegionKey,
    before: BTreeSet<Element>,
    mode: RegionMode,
    scripted: bool,
    clear_on_click: bool,
    moved: bool,
    exceeded: bool,
}
impl RegionDrag {
    pub(super) fn extend(&mut self, pos: Pos2) {
        if !pos.is_finite() {
            return;
        }
        let pos = pos.clamp(self.projection.rect.min, self.projection.rect.max);
        self.moved |= self.start.distance(pos) >= REGION_DRAG_PIXELS;
        if self.key.lasso {
            if self.shape.lasso.last().is_some_and(|last| last.distance(pos) < 2.0) {
                return;
            }
            if self.shape.lasso.len() >= MAX_REGION_POINTS {
                self.exceeded = true;
                return;
            }
            self.shape.extend(pos);
        } else {
            self.shape = crate::selection::BoxSel::from_drag(self.start, pos)
        }
    }
    pub(super) fn current(&self, app: &SolveApp, proj: &Proj) -> bool {
        app.preferences.forms_region_selection
            && RegionKey::current(app).as_ref() == Some(&self.key)
            && self.before == app.forms_prototype.elements
            && self.projection.cam == app.cam
            && self.projection.cam == proj.cam
            && self.projection.vp == proj.vp
            && self.projection.rect == proj.rect
            && app.viewport.rect == Some(proj.rect)
    }
}

#[derive(Default)]
pub(super) struct RegionJobs {
    latest: u64,
    error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<RegionPending>,
    #[cfg(not(target_arch = "wasm32"))]
    queued: Option<RegionRequest>,
}
#[cfg(not(target_arch = "wasm32"))]
type RegionWork = Box<dyn FnOnce(Arc<std::sync::atomic::AtomicBool>) -> RegionResult + Send + 'static>;
#[cfg(not(target_arch = "wasm32"))]
struct RegionRequest {
    serial: u64,
    drag: RegionDrag,
    work: RegionWork,
}
#[cfg(not(target_arch = "wasm32"))]
struct RegionPending {
    serial: u64,
    drag: RegionDrag,
    done: std::sync::mpsc::Receiver<RegionResult>,
    stop: RegionStop,
    #[cfg(test)]
    thread: Option<std::thread::JoinHandle<()>>,
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct RegionStop(Arc<std::sync::atomic::AtomicBool>);
#[cfg(not(target_arch = "wasm32"))]
impl RegionStop {
    fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    fn cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for RegionStop {
    fn drop(&mut self) {
        self.cancel();
    }
}
#[cfg(not(target_arch = "wasm32"))]
struct WorkerPermit {
    active: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(not(target_arch = "wasm32"))]
impl WorkerPermit {
    fn acquire(active: &Arc<std::sync::atomic::AtomicBool>) -> Option<Self> {
        use std::sync::atomic::Ordering;
        active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Self { active: active.clone() })
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.active.store(false, std::sync::atomic::Ordering::Release)
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn worker_flag() -> Arc<std::sync::atomic::AtomicBool> {
    static ACTIVE: std::sync::OnceLock<Arc<std::sync::atomic::AtomicBool>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(|| Arc::new(std::sync::atomic::AtomicBool::new(false))).clone()
}

/// Cancellation retains the receiver to drain old work. The process-wide permit
/// is owned by the worker, so dropping an editor/document cannot bypass the cap.
pub(super) fn cancel_region_work(app: &mut SolveApp) {
    app.forms_prototype.region = None;
    let jobs = &mut app.forms_prototype.region_jobs;
    jobs.latest = jobs.latest.wrapping_add(1);
    jobs.error = None;
    #[cfg(not(target_arch = "wasm32"))]
    {
        jobs.queued = None;
        if let Some(pending) = &jobs.pending {
            pending.stop.cancel();
        }
    }
}
pub(super) fn drain_region(app: &mut SolveApp) {
    if app.session.forms_prototype.is_none() {
        cancel_region_work(app)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = &mut app.forms_prototype.region_jobs;
        if jobs.pending.as_ref().is_some_and(|pending| pending.stop.cancelled())
            && jobs.pending.as_ref().is_some_and(|pending| !matches!(pending.done.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)))
        {
            jobs.pending = None
        }
    }
}
pub(super) fn begin_region(app: &mut SolveApp, proj: &Proj, pos: Pos2, mods: egui::Modifiers) -> bool {
    let Some(drag) = region_intent(app, proj, pos, mods) else { return false };
    cancel_region_work(app);
    app.forms_prototype.region = Some(drag);
    true
}

pub(super) fn region_intent(app: &SolveApp, proj: &Proj, pos: Pos2, mods: egui::Modifiers) -> Option<RegionDrag> {
    if !app.preferences.forms_region_selection || !pos.is_finite() || !proj.rect.contains(pos) {
        return None;
    }
    let key = RegionKey::current(app)?;
    let mode = if mods.ctrl || mods.command {
        RegionMode::Toggle
    } else if mods.shift || app.preferences.forms_greedy_selection {
        RegionMode::Add
    } else {
        RegionMode::Replace
    };
    let shape = if key.lasso { crate::selection::BoxSel::lasso(&[pos]) } else { crate::selection::BoxSel::from_drag(pos, pos) };
    Some(RegionDrag {
        start: pos,
        shape,
        projection: *proj,
        key,
        before: app.forms_prototype.elements.clone(),
        mode,
        scripted: app.synthetic_primary_press,
        clear_on_click: !(mods.shift || mods.ctrl || mods.command),
        moved: false,
        exceeded: false,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn accept_region_intent(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj, drag: RegionDrag, held: bool) {
    if drag.current(app, proj) {
        if held {
            app.forms_prototype.region = Some(drag);
        } else {
            release_region(app, ctx, proj, drag);
        }
    }
}
fn merge_region(before: &BTreeSet<Element>, found: &BTreeSet<Element>, mode: RegionMode) -> BTreeSet<Element> {
    match mode {
        RegionMode::Replace => found.clone(),
        RegionMode::Add => before.union(found).copied().collect(),
        RegionMode::Toggle => before.symmetric_difference(found).copied().collect(),
    }
}
fn logical_edges(faces: &[Vec<usize>]) -> BTreeSet<Element> {
    faces.iter().flat_map(|face| face.iter().zip(face.iter().cycle().skip(1)).take(face.len()).map(|(&a, &b)| edge(a, b))).collect()
}
fn valid_region(app: &SolveApp, found: &BTreeSet<Element>) -> bool {
    live_cad::selection_geometry(app).and_then(|exact| exact.logical.as_ref().ok()).is_some_and(|logical| {
        logical.accepts(found, app.forms_prototype.pick)
            && live_cad::selection_allowed(app).is_some_and(|allowed| found.iter().all(|element| allowed.contains(element)))
    })
}
#[cfg(test)]
fn valid_region_owned(app: &SolveApp, found: &BTreeSet<Element>, owners: Option<&[live_cad::ExactFaceOwner]>) -> bool {
    let Some(form) = &app.session.forms_prototype else { return false };
    if found.len() > MAX_REGION_USES
        || form.logical.faces.len() > MAX_REGION_USES
        || form.logical.faces.iter().try_fold(0_usize, |n, face| n.checked_add(face.len())).is_none_or(|uses| uses > MAX_REGION_USES)
    {
        return false;
    }
    let vertices: BTreeSet<_> =
        form.logical.faces.iter().flatten().copied().filter(|&id| id < form.logical.points.len() && id < form.surface.vertices.len()).collect();
    let edges = logical_edges(&form.logical.faces);
    let mut owned = BTreeSet::new();
    let mapped = owners.is_some_and(|owners| {
        owners.len() <= MAX_REGION_USES
            && owners.iter().all(|owner| match owner {
                live_cad::ExactFaceOwner::Face(id) => {
                    owned.insert(Element::Face(*id));
                    form.logical.faces.get(*id).is_some_and(|face| !face.is_empty())
                }
                live_cad::ExactFaceOwner::Surface => {
                    owned.insert(Element::Surface);
                    form.profile.is_some()
                }
                live_cad::ExactFaceOwner::Unmapped => false,
            })
    });
    found.iter().all(|e| {
        let filter = match app.forms_prototype.pick {
            Pick::All => true,
            Pick::Vertex => matches!(e, Element::Vertex(_)),
            Pick::Edge => matches!(e, Element::Edge(_)),
            Pick::Face => matches!(e, Element::Face(_) | Element::Surface),
        };
        filter
            && match e {
                Element::Surface => mapped && owned.contains(e) && form.profile.is_some(),
                Element::Face(id) => mapped && owned.contains(e) && form.logical.faces.get(*id).is_some_and(|face| !face.is_empty()),
                Element::Vertex(id) => vertices.contains(id) && form.selectable_control(*id) && live_cad::handle_available(app, *id),
                Element::Edge([a, b]) => *a < *b && *b < form.logical.points.len() && *b < form.surface.vertices.len() && edges.contains(e),
            }
    })
}
fn report_region_error(app: &mut SolveApp, error: String) {
    app.forms_prototype.region_jobs.error = Some(error.clone());
    app.set_status(solidcraft_ui_text::format("forms.prototype.region_failed", &[("error", &error)]), true);
}
fn finish_region(app: &mut SolveApp, proj: &Proj, drag: RegionDrag, result: RegionResult) {
    if !drag.current(app, proj) {
        return;
    }
    let next = if !drag.moved {
        if !drag.clear_on_click {
            return;
        }
        BTreeSet::new()
    } else if drag.exceeded {
        let error = app.tr("forms.prototype.region_limit").to_owned();
        report_region_error(app, error);
        return;
    } else {
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                report_region_error(app, error);
                return;
            }
        };
        if !valid_region(app, &found) {
            let error = app.tr("forms.prototype.region_invalid").to_owned();
            report_region_error(app, error);
            return;
        }
        merge_region(&drag.before, &found, drag.mode)
    };
    if next != app.forms_prototype.elements {
        let active = if matches!(drag.mode, RegionMode::Replace) { None } else { active(app).filter(|element| next.contains(element)) };
        app.forms_prototype.elements = next;
        app.forms_prototype.selection = State { active, ..Default::default() };
        app.forms_prototype.pivot = None;
        sync(app);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn enqueue_region(app: &mut SolveApp, drag: RegionDrag, work: RegionWork) {
    cancel_region_work(app);
    let jobs = &mut app.forms_prototype.region_jobs;
    jobs.queued = Some(RegionRequest { serial: jobs.latest, drag, work });
}
#[cfg(not(target_arch = "wasm32"))]
fn launch_region(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj, flag: &Arc<std::sync::atomic::AtomicBool>) {
    if app.forms_prototype.region_jobs.pending.is_some() || app.forms_prototype.region_jobs.queued.is_none() {
        return;
    }
    let Some(permit) = WorkerPermit::acquire(flag) else { return };
    let Some(request) = app.forms_prototype.region_jobs.queued.take() else { return };
    let RegionRequest { serial, drag, work } = request;
    let (tx, done) = std::sync::mpsc::channel();
    let stop = RegionStop::default();
    let worker_stop = stop.0.clone();
    let wake = ctx.clone();
    match std::thread::Builder::new().name("form-region-selection".into()).spawn(move || {
        let _permit = permit;
        let result = work(worker_stop);
        let _ = tx.send(result);
        wake.request_repaint();
    }) {
        Ok(thread) => {
            #[cfg(not(test))]
            drop(thread);
            app.forms_prototype.region_jobs.pending = Some(RegionPending {
                serial,
                drag,
                done,
                stop,
                #[cfg(test)]
                thread: Some(thread),
            });
        }
        Err(error) => finish_region(app, proj, drag, Err(error.to_string())),
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn poll_region_jobs(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj, flag: &Arc<std::sync::atomic::AtomicBool>, allow_launch: bool) {
    let stale = app
        .forms_prototype
        .region_jobs
        .pending
        .as_ref()
        .is_some_and(|pending| pending.serial != app.forms_prototype.region_jobs.latest || !pending.drag.current(app, proj));
    if stale && let Some(pending) = &app.forms_prototype.region_jobs.pending {
        pending.stop.cancel();
    }
    let result = app.forms_prototype.region_jobs.pending.as_ref().map(|pending| pending.done.try_recv());
    let result = match result {
        Some(Ok(result)) => Some(result),
        Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => Some(Err(app.tr("forms.prototype.region_worker_lost").to_owned())),
        _ => None,
    };
    if let Some(result) = result
        && let Some(pending) = app.forms_prototype.region_jobs.pending.take()
        && !pending.stop.cancelled()
        && pending.serial == app.forms_prototype.region_jobs.latest
    {
        finish_region(app, proj, pending.drag, result)
    }
    if app
        .forms_prototype
        .region_jobs
        .queued
        .as_ref()
        .is_some_and(|queued| queued.serial != app.forms_prototype.region_jobs.latest || !queued.drag.current(app, proj))
    {
        app.forms_prototype.region_jobs.queued = None
    }
    if allow_launch && app.forms_prototype.region.is_none() && app.forms_prototype.drag.is_none() && app.forms_prototype.gizmo_drag.is_none() {
        launch_region(app, ctx, proj, flag)
    }
    if region_busy(app) {
        ctx.request_repaint_after(std::time::Duration::from_millis(16))
    }
}
pub(super) fn poll_region(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj, blocked: bool) {
    if blocked {
        cancel_region_work(app)
    }
    #[cfg(not(target_arch = "wasm32"))]
    poll_region_jobs(app, ctx, proj, &worker_flag(), !blocked);
    #[cfg(target_arch = "wasm32")]
    let _ = (ctx, proj);
}
pub(super) fn region_can_cancel(app: &SolveApp) -> bool {
    if app.forms_prototype.region.is_some() {
        return true;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = &app.forms_prototype.region_jobs;
        jobs.queued.is_some() || jobs.pending.as_ref().is_some_and(|pending| pending.serial == jobs.latest && !pending.stop.cancelled())
    }
    #[cfg(target_arch = "wasm32")]
    false
}

/// The control channel targets this window even when another app has focus.
/// Only a gesture started by that channel is allowed to settle unfocused.
pub(super) fn region_lost_focus(app: &SolveApp, focused: bool) -> bool {
    if focused {
        return false;
    }
    if let Some(drag) = &app.forms_prototype.region {
        return !drag.scripted;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = &app.forms_prototype.region_jobs;
        if let Some(queued) = &jobs.queued {
            return !queued.drag.scripted;
        }
        if let Some(pending) = &jobs.pending {
            return pending.serial == jobs.latest && !pending.stop.cancelled() && !pending.drag.scripted;
        }
    }
    false
}
fn release_region(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj, mut drag: RegionDrag) {
    if !drag.current(app, proj) {
        return;
    }
    // Returning a rectangle to the press position is still a tiny click. A
    // lasso retains its complete bounds when its final point closes the loop.
    if !drag.exceeded && drag.shape.rect.width().max(drag.shape.rect.height()) < REGION_DRAG_PIXELS {
        drag.moved = false;
    }
    // Tiny clicks and overlong outlines never capture or classify geometry.
    if !drag.moved || drag.exceeded {
        finish_region(app, proj, drag, Ok(BTreeSet::new()));
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    match region_geometry::Query::capture(app, proj, &drag.shape) {
        Ok(query) => {
            enqueue_region(app, drag, Box::new(move |stop| query.run_cancellable(stop)));
            ctx.request_repaint()
        }
        Err(error) => finish_region(app, proj, drag, Err(error)),
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        let error = app.tr("forms.prototype.region_browser_unavailable").to_owned();
        finish_region(app, proj, drag, Err(error));
    }
}
/// Own the release/cancel frame too, so it cannot become a click or deformation.
pub(super) fn region_pointer(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj, input: &crate::navigation::CameraInput, blocked: bool) -> bool {
    let Some(mut drag) = app.forms_prototype.region.take() else { return false };
    if blocked
        || input.mode.is_some()
        || (!drag.scripted && !ui.input(|i| i.focused))
        || ui.input(|i| i.key_pressed(egui::Key::Escape))
        || !drag.current(app, proj)
    {
        return true;
    }
    // A busy frame can deliver the release and subsequent pointer movement together.
    // Close the outline at the release position, including when the pointer then leaves the view.
    let release = if input.primary_down {
        None
    } else {
        ui.input(|i| {
            i.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => Some(*pos),
                _ => None,
            })
        })
    };
    if let Some(pos) = release.or(input.hover) {
        drag.extend(pos)
    }
    if input.primary_down {
        app.forms_prototype.region = Some(drag)
    } else {
        release_region(app, ui.ctx(), proj, drag)
    }
    true
}
pub(super) fn region_busy(app: &SolveApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.forms_prototype.region_jobs.pending.is_some() || app.forms_prototype.region_jobs.queued.is_some()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        false
    }
}
pub(super) fn inspect_region(app: &SolveApp) -> serde_json::Value {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = &app.forms_prototype.region_jobs;
        json!({"supported":true,"dragging":app.forms_prototype.region.is_some(),"pending":jobs.pending.is_some(),"cancelled":jobs.pending.as_ref().is_some_and(|pending|pending.stop.cancelled()),"queued":jobs.queued.is_some(),"worker_active":worker_flag().load(std::sync::atomic::Ordering::Acquire),"serial":jobs.latest,"error":jobs.error})
    }
    #[cfg(target_arch = "wasm32")]
    {
        json!({"supported":false,"dragging":app.forms_prototype.region.is_some(),"error":app.forms_prototype.region_jobs.error})
    }
}
pub(super) fn show_region_status(app: &SolveApp, ui: &mut egui::Ui) {
    #[cfg(target_arch = "wasm32")]
    let _ = ui;
    if !region_busy(app) {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jobs = &app.forms_prototype.region_jobs;
        let waiting = jobs.queued.is_some() || jobs.pending.as_ref().is_some_and(|pending| pending.stop.cancelled());
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(app.tr(if waiting { "forms.prototype.region_waiting" } else { "forms.prototype.region_busy" }));
        });
    }
}
pub(super) fn paint_region(app: &SolveApp, ui: &egui::Ui) {
    let Some(drag) = &app.forms_prototype.region else { return };
    if !drag.moved {
        return;
    }
    let bx = &drag.shape;
    let t = Tokens::get();
    let col = if bx.crossing { t.box_crossing } else { t.box_window };
    let painter = ui.painter();
    if bx.is_lasso() {
        painter.add(egui::Shape::closed_line(bx.lasso.clone(), Stroke::new(1.2, col)));
    } else {
        painter.rect_filled(bx.rect, 0.0, col.gamma_multiply(0.12));
        if bx.crossing {
            let c = [bx.rect.left_top(), bx.rect.right_top(), bx.rect.right_bottom(), bx.rect.left_bottom()];
            for (a, b) in c.iter().zip(c.iter().cycle().skip(1)).take(4) {
                painter.add(egui::Shape::dashed_line(&[*a, *b], Stroke::new(1.2, col), 6.0, 4.0));
            }
        } else {
            painter.rect_stroke(bx.rect, 0.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_region_owns_escape_before_global_selection_clear() {
        let (mut app, proj, start) = region_app();
        app.home.open = false;
        let before = app.session.revision;
        let source = app.session.forms_prototype.as_ref().unwrap().inspect();
        assert!(!crate::forms_prototype::owns_key(&app, egui::Key::Escape));
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        assert!(crate::forms_prototype::owns_key(&app, egui::Key::Escape));
        assert!(!crate::forms_prototype::owns_key(&app, egui::Key::M));
        let ctx = egui::Context::default();
        let raw =
            egui::RawInput { screen_rect: Some(proj.rect), events: crate::test_support::key(egui::Key::Escape), focused: true, ..Default::default() };
        let _ = crate::test_support::frame(&ctx, raw, |ui| crate::toolbar::shortcuts(&mut app, ui.ctx()));
        assert!(app.forms_prototype.region.is_some(), "the viewport still owns cancellation");
        assert_eq!(app.session.revision, before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
        cancel_region_work(&mut app);
        assert!(!crate::forms_prototype::owns_key(&app, egui::Key::Escape));
    }

    #[test]
    fn release_position_keeps_empty_click_from_turning_into_a_region() {
        let (mut app, proj, start) = region_app();
        let end = start + egui::vec2(60.0, 50.0);
        app.forms_prototype.elements.insert(Element::Face(0));
        sync(&mut app);
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(proj.rect),
            events: vec![
                egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
                egui::Event::PointerMoved(end),
            ],
            ..Default::default()
        };
        let _ = crate::test_support::frame(&ctx, raw, |ui| {
            let response = ui.interact(proj.rect, ui.scope_id().with("forms_test_view"), egui::Sense::click());
            let input = crate::navigation::handle(&mut app, ui, proj.rect, &response, crate::navigation::NavCaps::MODEL);
            assert_eq!(input.hover, Some(end));
            assert!(!input.primary_down);
            assert!(region_pointer(&mut app, ui, &proj, &input, false));
        });
        assert!(app.forms_prototype.elements.is_empty());
        assert!(app.forms_prototype.region_jobs.error.is_none());
        assert!(!region_busy(&app));
    }

    #[test]
    fn tiny_and_overflow_regions_never_capture_or_queue_geometry() {
        let (mut app, proj, start) = region_app();
        let source = app.session.forms_prototype.as_ref().unwrap().inspect();
        app.forms_prototype.elements.insert(Element::Face(0));
        sync(&mut app);
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let drag = app.forms_prototype.region.take().unwrap();
        release_region(&mut app, &egui::Context::default(), &proj, drag);
        assert!(app.forms_prototype.elements.is_empty());
        assert!(app.forms_prototype.region_jobs.error.is_none());
        assert!(!region_busy(&app));
        app.forms_prototype.elements.insert(Element::Face(0));
        sync(&mut app);
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.extend(start + egui::vec2(60.0, 50.0));
        drag.extend(start + egui::vec2(1.0, 1.0));
        release_region(&mut app, &egui::Context::default(), &proj, drag);
        assert!(app.forms_prototype.elements.is_empty());
        assert!(app.forms_prototype.region_jobs.error.is_none());
        assert!(!region_busy(&app));
        app.forms_prototype.elements.insert(Element::Face(0));
        sync(&mut app);
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.moved = true;
        drag.exceeded = true;
        release_region(&mut app, &egui::Context::default(), &proj, drag);
        assert_eq!(app.forms_prototype.elements, [Element::Face(0)].into_iter().collect());
        assert_eq!(app.forms_prototype.region_jobs.error.as_deref(), Some(app.tr("forms.prototype.region_limit")));
        assert!(!region_busy(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pending_region(app: &mut SolveApp, proj: &Proj, start: Pos2) -> std::sync::mpsc::Sender<RegionResult> {
        assert!(begin_region(app, proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.extend(start + egui::vec2(60.0, 50.0));
        let (tx, done) = std::sync::mpsc::channel();
        let serial = app.forms_prototype.region_jobs.latest;
        app.forms_prototype.region_jobs.pending = Some(RegionPending { serial, drag, done, stop: RegionStop::default(), thread: None });
        tx
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn queued_drag(app: &mut SolveApp, proj: &Proj, start: Pos2) -> RegionDrag {
        assert!(begin_region(app, proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.extend(start + egui::vec2(60.0, 50.0));
        drag
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn only_control_channel_regions_can_settle_without_window_focus() {
        for scripted in [false, true] {
            let (mut app, proj, start) = region_app();
            app.forms_prototype.pick = Pick::Vertex;
            app.forms_prototype.elements.insert(Element::Vertex(0));
            sync(&mut app);
            let press =
                egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE };
            let mut raw = egui::RawInput { focused: false, ..Default::default() };
            if scripted {
                app.synthetic.push(press);
            } else {
                raw.events.push(press);
            }
            app.raw_input_hook(&mut raw);
            let tx = pending_region(&mut app, &proj, start);
            app.raw_input_hook(&mut egui::RawInput::default());
            assert!(!app.synthetic_primary_press, "the input origin flag must be limited to its own frame");
            tx.send(Ok([Element::Vertex(1)].into_iter().collect())).unwrap();
            let blocked = region_lost_focus(&app, false);
            assert_eq!(blocked, !scripted);
            poll_region(&mut app, &egui::Context::default(), &proj, blocked);
            assert_eq!(app.forms_prototype.elements, [Element::Vertex(usize::from(scripted))].into_iter().collect());
            assert!(!region_busy(&app));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn asynchronous_regions_reject_stale_camera_viewport_filter_source_selection_and_options() {
        for mutation in 0..10 {
            let (mut app, proj, start) = region_app();
            app.forms_prototype.pick = Pick::Vertex;
            app.forms_prototype.elements.insert(Element::Vertex(0));
            sync(&mut app);
            let tx = pending_region(&mut app, &proj, start);
            tx.send(Ok([Element::Vertex(1)].into_iter().collect())).unwrap();
            match mutation {
                0 => app.cam.yaw += 0.1,
                1 => app.viewport.rect = Some(proj.rect.shrink(1.0)),
                2 => app.forms_prototype.pick = Pick::Edge,
                3 => {
                    app.session.execute("form.prototype.create", &json!({"primitive":"box","levels":1})).unwrap();
                }
                4 => {
                    app.forms_prototype.elements.insert(Element::Vertex(2));
                }
                5 => app.forms_prototype.controls.tolerance = "0.02 mm".into(),
                6 => app.session.uid = app.session.uid.wrapping_add(1),
                7 => app.forms_prototype.context.version = app.forms_prototype.context.version.wrapping_add(1),
                8 => app.preferences.forms_greedy_selection = !app.preferences.forms_greedy_selection,
                _ => app.ui.lasso_select = !app.ui.lasso_select,
            }
            let selection = app.forms_prototype.elements.clone();
            let source = app.session.forms_prototype.as_ref().unwrap().inspect();
            let document = app.session.doc.clone();
            // A frozen Proj passed accidentally must not conceal the real camera or rect change.
            poll_region_jobs(&mut app, &egui::Context::default(), &proj, &Arc::new(std::sync::atomic::AtomicBool::new(false)), false);
            assert_eq!(app.forms_prototype.elements, selection, "case {mutation}");
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
            assert!(Arc::ptr_eq(&document, &app.session.doc));
            assert!(app.forms_prototype.region_jobs.pending.is_none());
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn asynchronous_region_errors_invalid_ids_and_cancellation_are_atomic() {
        for result in [Err("classifier refused".into()), Ok([Element::Vertex(1), Element::Vertex(usize::MAX)].into_iter().collect())] {
            let (mut app, proj, start) = region_app();
            app.forms_prototype.pick = Pick::Vertex;
            app.forms_prototype.elements.insert(Element::Vertex(0));
            sync(&mut app);
            let source = app.session.forms_prototype.as_ref().unwrap().inspect();
            let tx = pending_region(&mut app, &proj, start);
            tx.send(result).unwrap();
            poll_region_jobs(&mut app, &egui::Context::default(), &proj, &Arc::new(std::sync::atomic::AtomicBool::new(false)), false);
            assert_eq!(app.forms_prototype.elements, [Element::Vertex(0)].into_iter().collect());
            assert!(app.forms_prototype.region_jobs.error.is_some());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
        }
        let (mut app, proj, start) = region_app();
        app.forms_prototype.elements.insert(Element::Face(0));
        sync(&mut app);
        let before = app.forms_prototype.elements.clone();
        let tx = pending_region(&mut app, &proj, start);
        cancel_region_work(&mut app);
        assert!(app.forms_prototype.region_jobs.pending.as_ref().unwrap().stop.cancelled());
        tx.send(Ok([Element::Face(1)].into_iter().collect())).unwrap();
        poll_region_jobs(&mut app, &egui::Context::default(), &proj, &Arc::new(std::sync::atomic::AtomicBool::new(false)), true);
        assert_eq!(app.forms_prototype.elements, before);
        assert!(!region_busy(&app));
        assert!(app.forms_prototype.region_jobs.error.is_none());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn one_region_worker_drains_old_work_and_runs_only_the_latest_queued_request() {
        let (mut app, proj, start) = region_app();
        app.forms_prototype.pick = Pick::Vertex;
        app.forms_prototype.elements.insert(Element::Vertex(0));
        sync(&mut app);
        let source = app.session.forms_prototype.as_ref().unwrap().inspect();
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ctx = egui::Context::default();
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release_tx, release) = std::sync::mpsc::channel();
        let drag = queued_drag(&mut app, &proj, start);
        enqueue_region(
            &mut app,
            drag,
            Box::new(move |stop| {
                entered_tx.send(()).map_err(|e| e.to_string())?;
                release.recv().map_err(|e| e.to_string())?;
                assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
                Ok([Element::Vertex(2)].into_iter().collect())
            }),
        );
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        entered.recv().unwrap();
        let first = app.forms_prototype.region_jobs.pending.as_mut().unwrap().thread.take().unwrap();
        let (superseded_tx, superseded) = std::sync::mpsc::channel();
        let drag = queued_drag(&mut app, &proj, start);
        enqueue_region(
            &mut app,
            drag,
            Box::new(move |_| {
                superseded_tx.send(()).map_err(|e| e.to_string())?;
                Ok(BTreeSet::new())
            }),
        );
        let drag = queued_drag(&mut app, &proj, start);
        enqueue_region(&mut app, drag, Box::new(|_| Ok([Element::Vertex(1)].into_iter().collect())));
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        assert!(app.forms_prototype.region_jobs.pending.as_ref().unwrap().stop.cancelled());
        assert!(app.forms_prototype.region_jobs.queued.is_some());
        assert!(WorkerPermit::acquire(&flag).is_none());
        release_tx.send(()).unwrap();
        first.join().unwrap();
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        let latest = app.forms_prototype.region_jobs.pending.as_mut().unwrap().thread.take().unwrap();
        latest.join().unwrap();
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        assert!(superseded.try_recv().is_err());
        assert_eq!(app.forms_prototype.elements, [Element::Vertex(1)].into_iter().collect());
        assert!(!region_busy(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cancelled_region_query_never_returns_a_partial_selection() {
        let (app, proj, _) = region_app();
        let region = crate::selection::BoxSel::from_drag(proj.rect.right_bottom(), proj.rect.left_top());
        let query = region_geometry::Query::capture(&app, &proj, &region).unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(true));
        assert_eq!(query.run_cancellable(stop).unwrap_err(), "Form selection cancelled");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn resetting_editor_state_does_not_release_a_running_region_worker_permit() {
        let (mut app, proj, start) = region_app();
        let ctx = egui::Context::default();
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release_tx, release) = std::sync::mpsc::channel();
        let drag = queued_drag(&mut app, &proj, start);
        enqueue_region(
            &mut app,
            drag,
            Box::new(move |stop| {
                entered_tx.send(()).map_err(|e| e.to_string())?;
                release.recv().map_err(|e| e.to_string())?;
                assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
                Ok(BTreeSet::new())
            }),
        );
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        entered.recv().unwrap();
        let old = app.forms_prototype.region_jobs.pending.as_mut().unwrap().thread.take().unwrap();
        app.forms_prototype = Default::default();
        live_cad::install_for_test(&mut app);
        let drag = queued_drag(&mut app, &proj, start);
        enqueue_region(&mut app, drag, Box::new(|_| Ok([Element::Vertex(0)].into_iter().collect())));
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        assert!(app.forms_prototype.region_jobs.pending.is_none());
        assert!(app.forms_prototype.region_jobs.queued.is_some());
        assert!(WorkerPermit::acquire(&flag).is_none());
        release_tx.send(()).unwrap();
        old.join().unwrap();
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        let next = app.forms_prototype.region_jobs.pending.as_mut().unwrap().thread.take().unwrap();
        next.join().unwrap();
        poll_region_jobs(&mut app, &ctx, &proj, &flag, true);
        assert_eq!(app.forms_prototype.elements, [Element::Vertex(0)].into_iter().collect());
        assert!(!region_busy(&app));
    }
    #[test]
    fn region_face_validation_respects_partitioned_profiles_and_their_logical_edges() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let body = app.session.model.state().bodies[0].clone();
        let face = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|plane| plane.normal().dot(Vec3::Z) > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":face,"segments":[2,2],"partition":true})).unwrap();
        let cells = [live_cad::ExactFaceOwner::Face(0), live_cad::ExactFaceOwner::Face(1)];
        let whole = [live_cad::ExactFaceOwner::Surface];
        assert!(valid_region_owned(&app, &[Element::Face(0)].into_iter().collect(), Some(&cells)));
        assert!(!valid_region_owned(&app, &[Element::Surface].into_iter().collect(), Some(&cells)));
        // A requested partition can fall back to an unpartitioned fitted skin.
        assert!(valid_region_owned(&app, &[Element::Surface].into_iter().collect(), Some(&whole)));
        assert!(!valid_region_owned(&app, &[Element::Face(0)].into_iter().collect(), Some(&whole)));
        assert!(!valid_region_owned(&app, &[Element::Face(0)].into_iter().collect(), None));
        let points = &app.session.forms_prototype.as_ref().unwrap().logical.faces[0];
        let selected_edge = edge(points[0], points[1]);
        live_cad::install_for_test(&mut app);
        assert!(valid_region(&app, &[selected_edge].into_iter().collect()));
        let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
        let proj = crate::viewport::projection(&app, rect);
        app.viewport.rect = Some(rect);
        assert!(begin_region(&mut app, &proj, rect.left_top() + egui::vec2(1.0, 1.0), egui::Modifiers::NONE));
        let drag = app.forms_prototype.region.take().unwrap();
        let revision = app.session.forms_prototype.as_ref().unwrap().source_revision();
        app.session.forms_prototype.as_mut().unwrap().profile.as_mut().unwrap().partition = false;
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().source_revision(), revision);
        assert!(!drag.current(&app, &proj));
        assert!(valid_region_owned(&app, &[Element::Surface].into_iter().collect(), Some(&whole)));
        assert!(!valid_region_owned(&app, &[Element::Face(0)].into_iter().collect(), Some(&whole)));
    }

    fn region_app() -> (SolveApp, Proj, Pos2) {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"box","levels":1})).unwrap();
        fit(&mut app);
        // Framing the camera does not prepare the accepted native source. Region
        // completion must validate even injected worker results against that source.
        let candidates = [Element::Vertex(0), Element::Vertex(1), Element::Vertex(2)].into_iter().collect();
        assert!(!valid_region(&app, &candidates));
        live_cad::install_for_test(&mut app);
        assert!(valid_region(&app, &candidates));
        let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
        let proj = crate::viewport::projection(&app, rect);
        app.viewport.rect = Some(rect);
        (app, proj, rect.left_top() + egui::vec2(5.0, 5.0))
    }
    #[test]
    fn region_modifiers_replace_add_or_toggle_logical_identities() {
        let a = [Element::Vertex(0), Element::Face(2)].into_iter().collect();
        let b = [Element::Vertex(0), edge(1, 2)].into_iter().collect();
        assert_eq!(merge_region(&a, &b, RegionMode::Replace), b);
        assert_eq!(merge_region(&a, &b, RegionMode::Add).len(), 3);
        assert_eq!(merge_region(&a, &b, RegionMode::Toggle), [Element::Face(2), edge(1, 2)].into_iter().collect());
    }
    #[test]
    fn region_changes_only_selection_and_errors_never_partially_apply() {
        let (mut app, proj, start) = region_app();
        app.forms_prototype.pick = Pick::Vertex;
        app.forms_prototype.elements = [Element::Vertex(0)].into_iter().collect();
        sync(&mut app);
        let doc = app.session.doc.clone();
        let source = app.session.forms_prototype.as_ref().unwrap().inspect();
        for result in [
            Err("classifier unavailable".to_owned()),
            Ok([Element::Vertex(1), Element::Vertex(usize::MAX)].into_iter().collect()),
            Ok([Element::Face(0)].into_iter().collect()),
        ] {
            assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
            let mut drag = app.forms_prototype.region.take().unwrap();
            drag.extend(start + egui::vec2(60.0, 50.0));
            finish_region(&mut app, &proj, drag, result);
            assert_eq!(app.forms_prototype.elements, [Element::Vertex(0)].into_iter().collect());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
            assert!(Arc::ptr_eq(&doc, &app.session.doc));
        }
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.extend(start + egui::vec2(60.0, 50.0));
        finish_region(&mut app, &proj, drag, Ok([Element::Vertex(1)].into_iter().collect()));
        assert_eq!(app.forms_prototype.selected, [1].into_iter().collect());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), source);
        assert!(Arc::ptr_eq(&doc, &app.session.doc));
    }
    #[test]
    fn empty_click_never_deforms_and_legacy_preference_remains_reachable() {
        let (mut app, proj, start) = region_app();
        assert!(surface::nearest(&app, &proj, start).is_none());
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        for enabled in [false, true] {
            app.preferences.forms_region_selection = enabled;
            app.forms_prototype.elements = [Element::Face(0)].into_iter().collect();
            sync(&mut app);
            begin_drag(&mut app, &proj, start, egui::Modifiers::NONE);
            assert_eq!(app.forms_prototype.region.is_some(), enabled);
            assert!(app.forms_prototype.drag.is_none());
            if let Some(drag) = app.forms_prototype.region.take() {
                finish_region(&mut app, &proj, drag, Err("must not classify a click".into()));
            }
            assert!(app.forms_prototype.elements.is_empty());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        }
    }
    #[test]
    fn region_tracks_window_crossing_lasso_and_bounds_hostile_outlines() {
        let (mut app, proj, start) = region_app();
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.extend(start + egui::vec2(20.0, 20.0));
        assert!(!drag.shape.crossing);
        drag.extend(start - egui::vec2(3.0, 0.0));
        assert!(drag.shape.crossing);
        assert!(!drag.current(&app, &crate::viewport::projection(&app, proj.rect.shrink(1.0))));
        app.ui.lasso_select = true;
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.extend(egui::pos2(f32::NAN, f32::INFINITY));
        assert_eq!(drag.shape.lasso.len(), 1);
        for i in 0..MAX_REGION_POINTS + 10 {
            drag.extend(start + egui::vec2(if i % 2 == 0 { 10.0 } else { 20.0 }, 20.0));
        }
        assert!(drag.exceeded);
        assert_eq!(drag.shape.lasso.len(), MAX_REGION_POINTS);
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        finish_region(&mut app, &proj, drag, Ok([Element::Face(0)].into_iter().collect()));
        assert!(app.forms_prototype.elements.is_empty());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
    }
    #[test]
    fn source_filter_and_external_selection_changes_cancel_pending_region() {
        let (mut app, proj, start) = region_app();
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let mut drag = app.forms_prototype.region.take().unwrap();
        drag.extend(start + egui::vec2(50.0, 30.0));
        app.forms_prototype.pick = Pick::Edge;
        assert!(!drag.current(&app, &proj));
        app.forms_prototype.pick = Pick::All;
        app.forms_prototype.elements.insert(Element::Face(0));
        assert!(!drag.current(&app, &proj));
        finish_region(&mut app, &proj, drag, Ok(BTreeSet::new()));
        assert_eq!(app.forms_prototype.elements, [Element::Face(0)].into_iter().collect());
        assert!(begin_region(&mut app, &proj, start, egui::Modifiers::NONE));
        let drag = app.forms_prototype.region.take().unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face"})).unwrap();
        assert!(!drag.current(&app, &proj));
    }
    #[test]
    fn ranges_include_every_shortest_route_and_loops_stop_at_poles() {
        let faces: Vec<_> = (0..3).flat_map(|j| (0..3).map(move |i| vec![j * 4 + i, j * 4 + i + 1, (j + 1) * 4 + i + 1, (j + 1) * 4 + i])).collect();
        let selected = range(&faces, Element::Vertex(0), Element::Vertex(15)).unwrap();
        assert_eq!(selected.len(), 16);
        let selected = range(&faces, Element::Face(0), Element::Face(8)).unwrap();
        assert_eq!(selected.len(), 9);
        assert_eq!(edge_loop(&faces, edge(1, 5)).len(), 3);
        assert!(range(&faces, Element::Vertex(0), Element::Face(0)).is_none());
        let graph = graph(&faces, edge(0, 1)).unwrap();
        assert_eq!(distances(&graph, edge(0, 1)).len(), 24);
    }
}
