//! A release preview of the actual fitted CAD body, separate from saved design state.
use super::*;
type WorkerOutcome = Result<Computed, String>;
const MAX_SELECTION_ELEMENTS: usize = 262_144;
const MAX_DISPLAY_TRIANGLES: usize = 2_000_000;
const MAX_DISPLAY_POINTS: usize = 8_000_000;

#[cfg(not(target_arch = "wasm32"))]
struct PreviewPermit {
    gate: Arc<std::sync::atomic::AtomicBool>,
    wake: egui::Context,
}
#[cfg(not(target_arch = "wasm32"))]
impl PreviewPermit {
    fn acquire(gate: Arc<std::sync::atomic::AtomicBool>, wake: egui::Context) -> Option<Self> {
        use std::sync::atomic::Ordering;
        gate.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Self { gate, wake })
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for PreviewPermit {
    fn drop(&mut self) {
        // Dropping the editor's receiver does not cancel an in-flight fit.
        // Release only when its actual worker (or unspawned closure) exits.
        self.gate.store(false, std::sync::atomic::Ordering::Release);
        self.wake.request_repaint();
    }
}
#[cfg(all(not(test), not(target_arch = "wasm32")))]
fn preview_worker_gate() -> Arc<std::sync::atomic::AtomicBool> {
    static ACTIVE: std::sync::OnceLock<Arc<std::sync::atomic::AtomicBool>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(|| Arc::new(std::sync::atomic::AtomicBool::new(false))).clone()
}

/// A source identity is independent of cached meshes and of equal revision
/// counters in a different editor. Context includes document placement/support.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectionIdentity {
    document: u64,
    source: (u64, u64),
    context: u64,
    tolerance: u64,
    partition: Option<bool>,
}
impl SelectionIdentity {
    fn current(app: &SolveApp) -> Option<Self> {
        let form = app.session.forms_prototype.as_ref()?;
        Some(Self {
            document: app.session.uid,
            source: form.source_revision(),
            context: context::key(app),
            tolerance: controls::tolerance(app)?.to_bits(),
            partition: form.profile.as_ref().map(|profile| profile.partition),
        })
    }
    fn same_draft(&self, other: &Self) -> bool {
        self.document == other.document && self.source.0 == other.source.0 && self.context == other.context && self.partition == other.partition
    }
}

/// Logical identities come from the engine's own layout/selectability rules,
/// once on the CAD worker. Display triangles cannot add an identity here.
#[derive(Clone)]
pub(super) struct LogicalSelection {
    pub(super) allowed: Arc<BTreeSet<Element>>,
    without_fixed: Arc<BTreeSet<Element>>,
    pub(super) active: BTreeSet<Element>,
    valid_faces: BTreeSet<Element>,
}
impl LogicalSelection {
    fn build(definition: &solidcraft_engine::doc::forms::Definition, owners: &[ExactFaceOwner]) -> Result<Self, String> {
        if owners.len() > MAX_SELECTION_ELEMENTS {
            return Err("Form selection exceeds its native face budget".into());
        }
        fn bounded_faces(faces: &[Vec<usize>], limit: usize) -> Result<(), String> {
            if faces.len() > limit {
                return Err("Form selection exceeds its logical face budget".into());
            }
            faces
                .iter()
                .try_fold(0usize, |uses, face| {
                    uses.checked_add(face.len()).filter(|uses| *uses <= limit).ok_or("Form selection exceeds its logical element budget")
                })
                .map(|_| ())
                .map_err(str::to_owned)
        }
        bounded_faces(&definition.cage.faces, 2_000_000)?;
        if definition.cage.points.len() > 600_000 || definition.cage.faces.len() > 500_000 {
            return Err("Form selection exceeds its source point budget".into());
        }
        if let Some(hierarchy) = &definition.hierarchy {
            bounded_faces(&hierarchy.base.faces, 2_000_000)?;
            if hierarchy.base.points.len() > 600_000
                || hierarchy.base.faces.len() > 500_000
                || hierarchy.leaves.len() > MAX_SELECTION_ELEMENTS
                || hierarchy.controls.len() > 600_000
                || hierarchy.details.len() > 5
                || hierarchy.sharpness.len() > 5
                || hierarchy.dissolved_edges.len() > MAX_SELECTION_ELEMENTS
            {
                return Err("Form selection exceeds its hierarchy budget".into());
            }
            for details in &hierarchy.details {
                if details.len() > 600_000 {
                    return Err("Form selection exceeds its detail budget".into());
                }
            }
            for sharpness in &hierarchy.sharpness {
                if sharpness.edges.len() > 2_000_000 || sharpness.vertices.len() > 600_000 {
                    return Err("Form selection exceeds its crease budget".into());
                }
            }
        }
        let form = solidcraft_engine::forms_prototype::Form::from_definition(definition.clone(), 0, None).map_err(|error| error.to_string())?;
        bounded_faces(&form.logical.faces, MAX_SELECTION_ELEMENTS)?;
        let whole = owners.contains(&ExactFaceOwner::Surface);
        if whole && owners.iter().any(|owner| matches!(owner, ExactFaceOwner::Face(_))) {
            return Err("the exact Form preview mixes whole-surface and cell ownership".into());
        }
        let mut allowed = BTreeSet::new();
        let mut active = BTreeSet::new();
        for (id, face) in form.logical.faces.iter().enumerate().filter(|(_, face)| !face.is_empty()) {
            if !whole {
                allowed.insert(Element::Face(id));
                active.insert(Element::Face(id));
            }
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                if a >= form.logical.points.len() || b >= form.logical.points.len() {
                    return Err("the logical Form layout contains a missing vertex".into());
                }
                if form.selectable_control(a) {
                    allowed.insert(Element::Vertex(a));
                }
                active.insert(Element::Vertex(a));
                active.insert(Element::Edge([a.min(b), a.max(b)]));
                allowed.insert(Element::Edge([a.min(b), a.max(b)]));
            }
        }
        if whole {
            allowed.insert(Element::Surface);
        }
        if whole {
            active.insert(Element::Surface);
        }
        for (edge, _) in &form.surface.boundaries {
            active.insert(Element::Edge([edge[0].min(edge[1]), edge[0].max(edge[1])]));
        }
        let mut valid_faces = BTreeSet::new();
        for owner in owners {
            let element = match owner {
                ExactFaceOwner::Face(id) => Element::Face(*id),
                ExactFaceOwner::Surface if form.profile.is_some() => Element::Surface,
                _ => {
                    valid_faces.clear();
                    break;
                }
            };
            if !allowed.contains(&element) {
                valid_faces.clear();
                break;
            }
            valid_faces.insert(element);
        }
        let allowed = Arc::new(allowed);
        let without_fixed = if let Some(profile) = &form.profile {
            let boundary = profile.boundary();
            Arc::new(allowed.iter().copied().filter(|element| !matches!(element, Element::Vertex(id) if boundary.contains(id))).collect())
        } else {
            allowed.clone()
        };
        Ok(Self { allowed, without_fixed, active, valid_faces })
    }
    fn allowed_for(&self, fixed_available: bool) -> &Arc<BTreeSet<Element>> {
        if fixed_available { &self.allowed } else { &self.without_fixed }
    }
    pub(super) fn accepts(&self, found: &BTreeSet<Element>, pick: Pick) -> bool {
        found.len() <= MAX_SELECTION_ELEMENTS
            && found.iter().all(|element| {
                let filter = match pick {
                    Pick::All => true,
                    Pick::Vertex => matches!(element, Element::Vertex(_)),
                    Pick::Edge => matches!(element, Element::Edge(_)),
                    Pick::Face => matches!(element, Element::Face(_) | Element::Surface),
                };
                filter
                    && self.allowed.contains(element)
                    && (!matches!(element, Element::Face(_) | Element::Surface) || self.valid_faces.contains(element))
            })
    }
}

/// Bound every buffer from which MeshIndex allocates, including long display
/// edge polylines. This runs before indexing, on the producer worker.
pub(super) fn validate_display_mesh(mesh: &Mesh) -> Result<(), String> {
    if mesh.triangles.len() > MAX_DISPLAY_TRIANGLES
        || mesh.tri_face.len() > MAX_DISPLAY_TRIANGLES
        || mesh.positions.len() > MAX_DISPLAY_TRIANGLES * 3
        || mesh.normals.len() > MAX_DISPLAY_TRIANGLES * 3
        || mesh.edges.len() > MAX_DISPLAY_POINTS
        || mesh.edge_faces.len() > MAX_DISPLAY_POINTS
        || mesh.seams.len() > MAX_DISPLAY_POINTS
        || mesh.smooth_edges.len() > MAX_DISPLAY_POINTS
        || mesh.sharp_edges.len() > MAX_DISPLAY_POINTS
    {
        return Err("Form display geometry budget exceeded".into());
    }
    display_path_count(mesh.edges.iter().map(Vec::len))?;
    display_path_count(mesh.edge_faces.iter().map(Vec::len))?;
    Ok(())
}
fn display_path_count(mut lengths: impl Iterator<Item = usize>) -> Result<usize, String> {
    lengths.try_fold(0usize, |total, n| {
        total.checked_add(n).filter(|sum| *sum <= MAX_DISPLAY_POINTS).ok_or_else(|| "Form display path budget exceeded".to_owned())
    })
}
fn indexed_mesh(mesh: Mesh) -> Result<(Arc<Mesh>, Arc<solidcraft_engine::geom::MeshIndex>), String> {
    validate_display_mesh(&mesh)?;
    let index = Arc::new(solidcraft_engine::geom::MeshIndex::new(&mesh));
    Ok((Arc::new(mesh), index))
}
/// Native face ownership is independent of tessellation and of native order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExactFaceOwner {
    Face(usize),
    Surface,
    /// No exact source-to-logical contract covers this native face.
    Unmapped,
}
/// Exact edited B-rep, logical source routes and endpoints in the accepted
/// source's world placement. Coverage refusals remain explicit; display paths
/// and triangle IDs cannot fill a missing exact route.
#[derive(Clone)]
pub(super) struct ExactSelection {
    pub(super) body: solidcraft_engine::doc::kernel::Body,
    pub(super) faces: Vec<ExactFaceOwner>,
    pub(super) definition: Arc<solidcraft_engine::doc::forms::Definition>,
    pub(super) logical: Result<Arc<LogicalSelection>, String>,
    pub(super) guides: solidcraft_engine::doc::forms::SelectionGuides,
    pub(super) points: Result<std::collections::BTreeMap<usize, Vec3>, String>,
    frames: Arc<solidcraft_engine::doc::forms::SelectionFrames>,
}
impl ExactSelection {
    fn new(body: solidcraft_engine::doc::kernel::Body, definition: solidcraft_engine::doc::forms::Definition, owners: Option<&[usize]>) -> Self {
        let valid_owner = |id: usize| {
            definition.hierarchy.as_ref().map_or_else(
                || definition.cage.faces.get(id).is_some_and(|face| !face.is_empty()),
                |hierarchy| {
                    hierarchy.leaves.get(id).is_some_and(|leaf| leaf.active)
                        || (hierarchy.has_trim_wall() && id == hierarchy.leaves.len())
                        || hierarchy.is_quad_cap_face(id)
                },
            )
        };
        // The fit may fall back to one whole skin even when cells were requested.
        // Only actual native provenance decides the selectable unit.
        let whole_surface = definition.profile.is_some() && owners.is_none();
        let owners = owners.filter(|owners| owners.len() == body.face_count());
        let faces: Vec<_> = (0..body.face_count().min(MAX_SELECTION_ELEMENTS + 1))
            .map(|face| match owners.and_then(|owners| owners.get(face)).copied() {
                Some(owner) if valid_owner(owner) => ExactFaceOwner::Face(owner),
                _ if whole_surface => ExactFaceOwner::Surface,
                _ => ExactFaceOwner::Unmapped,
            })
            .collect();
        let logical = LogicalSelection::build(&definition, &faces).map(Arc::new);
        Self {
            body,
            faces,
            logical,
            definition: Arc::new(definition),
            guides: solidcraft_engine::doc::forms::SelectionGuides::Unmapped("exact logical guide routing was not requested".into()),
            points: Err("exact logical point routing was not requested".into()),
            frames: Arc::new(Default::default()),
        }
    }
    fn with_guides(mut self, guides: solidcraft_engine::doc::forms::SelectionGuides) -> Self {
        self.points = guides.points().map_err(|error| error.to_string());
        self.guides = guides;
        self
    }
    pub(super) fn frame(&self, element: Element) -> Option<[Vec3; 3]> {
        let logical = self.logical.as_ref().ok()?;
        if !logical.allowed.contains(&element) {
            return None;
        }
        let frame = match element {
            Element::Face(id) if logical.valid_faces.contains(&element) => self.frames.faces.get(&id),
            Element::Edge(edge) => self.frames.edges.get(&edge),
            Element::Vertex(id) => self.frames.points.get(&id),
            _ => None,
        }?;
        Some(frame.map(|axis| self.definition.placement.vector(axis)))
    }
    fn relocated(&self, placement: &solidcraft_engine::doc::forms::Placement) -> Result<Self, String> {
        placement.validate().map_err(|error| error.to_string())?;
        let mut relocated = self.clone();
        if self.definition.placement != *placement {
            let inverse =
                solidcraft_engine::doc::mat_inverse(&self.definition.placement.matrix()).ok_or("the CAD source placement is not invertible")?;
            let matrix = solidcraft_engine::doc::mat_mul(&placement.matrix(), &inverse);
            if solidcraft_engine::geom::brep::placement::Rigid::of_matrix(&matrix).is_none() {
                return Err("exact CAD placement requires the topology-preserving rigid path".into());
            }
            // The rigid-record path preserves native parameter and topology
            // indices; this moves exact geometry without fitting.
            relocated.body = solidcraft_engine::doc::kernel::transform_matrix(&self.body, matrix).map_err(|error| error.to_string())?;
            if relocated.body.face_count() != self.faces.len() {
                return Err("the rigid CAD placement lost native face provenance".into());
            }
            relocated.guides = self.guides.relocated(&self.definition.placement, placement).map_err(|error| error.to_string())?;
            relocated.points = relocated.guides.points().map_err(|error| error.to_string());
            Arc::make_mut(&mut relocated.definition).placement = placement.clone();
        }
        Ok(relocated)
    }
}
struct Computed {
    mesh: Arc<Mesh>,
    index: Arc<solidcraft_engine::geom::MeshIndex>,
    exact: ExactSelection,
    report: solidcraft_engine::doc::forms::ConversionReport,
    matched: Option<(solidcraft_engine::doc::forms::Definition, solidcraft_engine::geom::subdivision::matching::Report)>,
    profiled: Option<solidcraft_engine::doc::forms::Definition>,
}
struct Source {
    document: Arc<solidcraft_engine::doc::Document>,
    before: Arc<solidcraft_engine::doc::ModelState>,
    moves: std::collections::BTreeMap<u64, solidcraft_engine::doc::Mat>,
}
impl Source {
    fn world(&self) -> Result<solidcraft_engine::doc::ModelState, String> {
        let mut document = std::borrow::Cow::Borrowed(self.document.as_ref());
        if !self.moves.is_empty() {
            for occurrence in &mut document.to_mut().occurrences {
                if let Some(matrix) = self.moves.get(&occurrence.id) {
                    occurrence.transform = *matrix;
                }
            }
        }
        solidcraft_engine::doc::world_state(&document, &self.before).map_err(|e| e.to_string())
    }
}
fn compute_context(mut definition: solidcraft_engine::doc::forms::Definition, tolerance: f64, source: Option<Source>) -> WorkerOutcome {
    if let Some(profile) = &definition.profile {
        let state = source.ok_or("Form Surface preview requires its source document")?.world()?;
        let (profile, cage) = profile.recompute(&definition, &state).map_err(|e| e.to_string())?;
        definition.cage = cage;
        definition.profile = Some(Box::new(profile));
        let profile = definition.profile.as_ref().ok_or("Form Surface preview lost its source")?;
        let display = profile.display_with_body(&state, &definition.cage, &definition.placement, tolerance).map_err(|e| e.to_string())?;
        let frames = solidcraft_engine::doc::forms::SelectionFrames::from_profile(
            &definition,
            &display.patch,
            display.patch_roots.as_deref(),
            &display.guides,
        );
        let mut exact = ExactSelection::new(display.patch, definition.clone(), display.patch_roots.as_deref()).with_guides(display.guides);
        exact.frames = Arc::new(frames);
        let (mesh, index) = indexed_mesh(display.mesh)?;
        return Ok(Computed { mesh, index, exact, report: display.report, matched: None, profiled: Some(definition) });
    }
    if let (Some(matching), Some(source)) = (&definition.matching, source) {
        let (resolved, match_report) = matching.recompute(&definition, &source.world()?, tolerance).map_err(|e| e.to_string())?;
        let mut computed = compute_with_body(resolved.clone(), tolerance)?;
        computed.matched = Some((resolved, match_report));
        return Ok(computed);
    }
    compute_with_body(definition, tolerance)
}
fn key(app: &SolveApp, revision: u64, tolerance: f64) -> (u64, u64) {
    let revision = if app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some() || f.matching.is_some()) {
        revision ^ context::key(app).rotate_left(17)
    } else {
        revision
    };
    (revision, tolerance.to_bits())
}
struct Pending {
    key: (u64, u64),
    source: solidcraft_engine::doc::forms::Definition,
    identity: SelectionIdentity,
    done: std::sync::mpsc::Receiver<WorkerOutcome>,
}
#[derive(Default)]
pub(super) struct State {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    worker_gate: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    queued: bool,
    profile_context: Option<u64>,
    fixed_vertices: Vec<Vec3>,
    fixed_perimeter: Vec<Option<Vec<Vec3>>>,
    source: Option<Arc<solidcraft_engine::doc::forms::Definition>>,
    accepted_identity: Option<SelectionIdentity>,
    placement_reuses: u64,
    pending: Option<Pending>,
    requested: Option<(u64, u64)>,
    ready: Option<((u64, u64), Arc<Mesh>, solidcraft_engine::doc::forms::ConversionReport)>,
    index: Option<Arc<solidcraft_engine::geom::MeshIndex>>,
    exact: Option<((u64, u64), Arc<ExactSelection>)>,
    pub(super) error: Option<String>,
}
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn compute_context_body(
    definition: solidcraft_engine::doc::forms::Definition,
    tolerance: f64,
) -> Result<(Arc<Mesh>, solidcraft_engine::doc::kernel::Body), String> {
    let computed = compute_with_body(definition, tolerance)?;
    Ok((computed.mesh, computed.exact.body))
}
fn compute_with_body(definition: solidcraft_engine::doc::forms::Definition, tolerance: f64) -> WorkerOutcome {
    let (body, report, guides, frames) = solidcraft_engine::doc::forms::convert_with_selection(&definition, tolerance).map_err(|e| e.to_string())?;
    let mut mesh = body.mesh(&solidcraft_engine::doc::kernel::MeshTolerance::chord(tolerance.min(0.05))).map_err(|e| e.to_string())?;
    if mesh.triangles.is_empty() || mesh.tri_face.len() != mesh.triangles.len() {
        return Err("the CAD preview has no usable face tessellation".into());
    }
    for face in &mut mesh.tri_face {
        *face = *report.patch_roots.get(*face as usize).ok_or("the CAD preview lost a logical Form face")? as u32;
    }
    mesh.seams.resize(mesh.edges.len(), false);
    for (i, faces) in mesh.edge_faces.iter_mut().enumerate() {
        let interior = faces.len() > 1;
        for face in faces.iter_mut() {
            *face = *report.patch_roots.get(*face as usize).ok_or("the CAD preview lost a logical edge neighbour")? as u32;
        }
        faces.sort_unstable();
        faces.dedup();
        if let Some(seam) = mesh.seams.get_mut(i) {
            *seam = interior && faces.len() == 1;
        }
    }
    let mut exact = ExactSelection::new(body, definition, Some(&report.patch_roots)).with_guides(guides);
    exact.frames = Arc::new(frames);
    let (mesh, index) = indexed_mesh(mesh)?;
    Ok(Computed { mesh, index, exact, report, matched: None, profiled: None })
}
/// Only the current accepted revision/context/tolerance can expose exact data.
/// A stale mesh or manually installed display-only cache supplies none.
pub(super) fn selection_geometry(app: &SolveApp) -> Option<&Arc<ExactSelection>> {
    mesh(app)?;
    let form = app.session.forms_prototype.as_ref()?;
    let tolerance = controls::tolerance(app)?;
    let live = &app.forms_prototype.live;
    let (given, exact) = live.exact.as_ref()?;
    (*given == key(app, form.revision, tolerance)
        && live.accepted_identity.as_ref() == Some(&SelectionIdentity::current(app)?)
        && live.source.as_ref().is_some_and(|source| Arc::ptr_eq(source, &exact.definition)))
    .then_some(exact)
}
/// The exact source proof already qualifies profile/context identity. Only the
/// accepted derived handle buffers can still be unavailable; inspect lengths
/// here, without repeating the deep profile comparison used by pending drags.
pub(super) fn selection_allowed(app: &SolveApp) -> Option<&Arc<BTreeSet<Element>>> {
    let logical = selection_geometry(app)?.logical.as_ref().ok()?;
    let form = app.session.forms_prototype.as_ref()?;
    let live = &app.forms_prototype.live;
    let fixed = form.profile.is_some()
        && live.profile_context == Some(context::key(app))
        && live.source.as_ref().is_some_and(|source| source.placement == form.placement)
        && live.fixed_vertices.len() == form.surface.vertices.len()
        && live.fixed_perimeter.len() == form.surface.boundaries.len();
    Some(logical.allowed_for(fixed))
}
pub(super) fn mesh(app: &SolveApp) -> Option<&Mesh> {
    let form = app.session.forms_prototype.as_ref()?;
    let tolerance = controls::tolerance(app)?;
    app.forms_prototype.live.ready.as_ref().filter(|(given, _, _)| *given == key(app, form.revision, tolerance)).map(|(_, m, _)| m.as_ref())
}
pub(super) fn report(app: &SolveApp) -> Option<&solidcraft_engine::doc::forms::ConversionReport> {
    mesh(app)?;
    app.forms_prototype.live.ready.as_ref().map(|(_, _, report)| report)
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    let live = &app.forms_prototype.live;
    let ready = mesh(app).is_some();
    let exact = selection_geometry(app).map(|exact| {
        json!({
            "native_faces":exact.body.face_count(),
            "logical_faces":exact.faces.iter().filter(|owner| matches!(owner, ExactFaceOwner::Face(_))).count(),
            "whole_surface_faces":exact.faces.iter().filter(|owner| matches!(owner, ExactFaceOwner::Surface)).count(),
            "unmapped_faces":exact.faces.iter().filter(|owner| matches!(owner, ExactFaceOwner::Unmapped)).count(),
            "logical_guides_available":!matches!(exact.guides, solidcraft_engine::doc::forms::SelectionGuides::Unmapped(_)),
            "logical_points_available":exact.points.is_ok(),
            "guide_coverage_error":match &exact.guides { solidcraft_engine::doc::forms::SelectionGuides::Unmapped(reason) => Some(reason), _ => None },
            "point_coverage_error":exact.points.as_ref().err()
        })
    });
    json!({"kind":if ready{"cad"}else{"subdivision"},"busy":busy(app),"queued":queued(live),"error":live.error,"placement_reuses":live.placement_reuses,"fit":if ready{live.ready.as_ref().map(|(_,_,r)|r)}else{None},"exact_selection":exact})
}
fn same_shape(form: &solidcraft_engine::forms_prototype::Form, source: &solidcraft_engine::doc::forms::Definition) -> bool {
    form.profile.is_none() && form.matching.is_none() && source.matching.is_none() && form.cage == source.cage && form.hierarchy == source.hierarchy
}
fn place_mesh(mesh: &mut Mesh, old: &solidcraft_engine::doc::forms::Placement, new: &solidcraft_engine::doc::forms::Placement) {
    for p in &mut mesh.positions {
        *p = new.point(old.local_point(*p));
    }
    for n in &mut mesh.normals {
        *n = new.vector(old.local_vector(*n));
    }
    for edge in &mut mesh.edges {
        for p in edge {
            *p = new.point(old.local_point(*p));
        }
    }
}
pub(super) fn busy(app: &SolveApp) -> bool {
    app.forms_prototype.live.pending.is_some() || queued(&app.forms_prototype.live)
}
fn queued(live: &State) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        live.queued
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = live;
        false
    }
}
fn fixed_profile_handles_current(app: &SolveApp) -> bool {
    let Some(form) = &app.session.forms_prototype else { return false };
    let live = &app.forms_prototype.live;
    let Some(source) = &live.source else { return false };
    form.profile.is_some()
        && form.profile == source.profile
        && form.placement == source.placement
        && live.profile_context == Some(context::key(app))
        && live.fixed_vertices.len() == form.surface.vertices.len()
        && live.fixed_perimeter.len() == form.surface.boundaries.len()
}
pub(super) fn handle_available(app: &SolveApp, id: usize) -> bool {
    let Some(form) = &app.session.forms_prototype else { return false };
    form.profile.as_ref().is_none_or(|profile| !profile.boundary().contains(&id) || fixed_profile_handles_current(app))
}
pub(super) fn restore_fixed_profile_handles(app: &mut SolveApp) {
    if !fixed_profile_handles_current(app) {
        return;
    }
    let Some(form) = &mut app.session.forms_prototype else { return };
    let Some(profile) = &form.profile else { return };
    if form.surface.mesh.positions.len() != form.surface.vertices.len() {
        return;
    }
    let boundary = profile.boundary();
    let fixed_vertices = &app.forms_prototype.live.fixed_vertices;
    let fixed_perimeter = &app.forms_prototype.live.fixed_perimeter;
    // A read-only paint must not copy a preview pinned by a worker. Only
    // obtain COW access when a fixed handle or its derived geometry differs.
    let needs_vertices = boundary.iter().any(|&id| {
        fixed_vertices.get(id).is_some_and(|point| {
            form.surface.vertices.get(id).is_some_and(|vertex| vertex != point)
                || form.surface.mesh.positions.get(id).is_some_and(|position| position != point)
        })
    });
    let needs_perimeter = form.surface.boundaries.iter().zip(fixed_perimeter).any(|(([a, b], curve), fixed)| {
        fixed.as_ref().map_or_else(
            || {
                [(*a, 0), (*b, curve.len().saturating_sub(1))]
                    .into_iter()
                    .any(|(id, endpoint)| form.surface.vertices.get(id).zip(curve.get(endpoint)).is_some_and(|(point, vertex)| point != vertex))
            },
            |fixed| curve != fixed,
        )
    });
    if !needs_vertices && !needs_perimeter {
        return;
    }
    let surface = Arc::make_mut(&mut form.surface);
    let mut changed = false;
    for id in boundary {
        if let (Some(vertex), Some(point)) = (surface.vertices.get_mut(id), fixed_vertices.get(id)) {
            *vertex = *point;
            if let Some(position) = surface.mesh.positions.get_mut(id)
                && *position != *point
            {
                *position = *point;
                changed = true;
            }
        }
    }
    for (([a, b], curve), fixed) in surface.boundaries.iter_mut().zip(fixed_perimeter) {
        if let Some(fixed) = fixed {
            if curve != fixed {
                curve.clone_from(fixed);
                changed = true;
            }
        } else {
            for (id, endpoint) in [(*a, 0), (*b, curve.len().saturating_sub(1))] {
                if let (Some(point), Some(vertex)) = (surface.vertices.get(id), curve.get_mut(endpoint))
                    && *vertex != *point
                {
                    *vertex = *point;
                    changed = true;
                }
            }
        }
    }
    if changed {
        // Profile.preview has one mesh vertex per logical control. Restore the
        // same derived geometry for rendering and ray tests, not just markers.
        let mesh = &mut surface.mesh;
        mesh.normals = vec![Vec3::ZERO; mesh.positions.len()];
        for triangle in &mesh.triangles {
            let Some([a, b, c]) = mesh.tri(triangle) else { continue };
            let normal = (b - a).cross(c - a);
            for &id in triangle {
                if let Some(target) = mesh.normals.get_mut(id as usize) {
                    *target += normal;
                }
            }
        }
        for normal in &mut mesh.normals {
            *normal = normal.normalized().unwrap_or(Vec3::Z);
        }
        surface.index = solidcraft_engine::geom::MeshIndex::new(mesh);
        app.forms_prototype.uploaded = 0;
    }
}
pub(super) fn occlusion(app: &SolveApp) -> Option<(&Mesh, &solidcraft_engine::geom::MeshIndex)> {
    Some((mesh(app)?, app.forms_prototype.live.index.as_deref()?))
}
#[cfg(any(test, not(target_arch = "wasm32")))]
pub(super) fn occlusion_snapshot(app: &SolveApp) -> Option<(Arc<Mesh>, Arc<solidcraft_engine::geom::MeshIndex>)> {
    mesh(app)?;
    let live = &app.forms_prototype.live;
    Some((live.ready.as_ref()?.1.clone(), live.index.as_ref()?.clone()))
}
fn accept(app: &mut SolveApp, tolerance: f64, mut source: solidcraft_engine::doc::forms::Definition, result: WorkerOutcome) {
    let profile_context = context::key(app);
    let Some(form) = app.session.forms_prototype.as_mut() else { return };
    let mut placed = false;
    let mut fixed_perimeter = Vec::new();
    let result = result.and_then(|mut computed| {
        if let Some(resolved) = computed.profiled.take() {
            form.refresh_profile_preview(resolved).map_err(|e| e.to_string())?;
            source = form.definition();
        } else if let Some((resolved, report)) = computed.matched.take() {
            form.refresh_match_preview(resolved, report).map_err(|e| e.to_string())?;
            source = form.definition();
        } else if source.placement != form.placement {
            computed.exact = computed.exact.relocated(&form.placement)?;
            place_mesh(Arc::make_mut(&mut computed.mesh), &source.placement, &form.placement);
            computed.index = Arc::new(solidcraft_engine::geom::MeshIndex::new(&computed.mesh));
            source.placement = form.placement.clone();
            placed = true;
        }
        if let Some(profile) = &form.profile {
            // A trimmed support's rectangular logical grid can have fixed
            // boundary controls outside the actual rim. Keep their identities,
            // but draw and pick their derived positions on the exact CAD guide
            // curves. The authored cage/rest and editable interior stay intact.
            if computed.mesh.edges.len() != form.surface.boundaries.len() {
                return Err("Form Surface display lost its logical guide edges".into());
            }
            let boundary = profile.boundary();
            let perimeter: BTreeSet<_> = form
                .cage
                .boundary_loops()
                .map_err(|e| e.to_string())?
                .iter()
                .flat_map(|ring| ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()).map(|(&a, &b)| [a.min(b), a.max(b)]))
                .collect();
            let mut vertices = form.surface.vertices.clone();
            for ((edge, _), curve) in form.surface.boundaries.iter().zip(&computed.mesh.edges) {
                fixed_perimeter.push(perimeter.contains(edge).then(|| curve.clone()));
                for (id, point) in [(edge[0], curve.first()), (edge[1], curve.last())] {
                    if !boundary.contains(&id) {
                        continue;
                    }
                    let Some(point) = point.filter(|point| point.is_finite()) else {
                        return Err("Form Surface display lost a finite boundary guide".into());
                    };
                    let Some(vertex) = vertices.get_mut(id) else {
                        return Err("Form Surface display lost a boundary control".into());
                    };
                    *vertex = *point;
                }
            }
            Arc::make_mut(&mut form.surface).vertices = vertices;
        }
        computed.exact.definition = Arc::new(source);
        Ok(computed)
    });
    let revision = form.revision;
    let fixed_vertices = form.profile.is_some().then(|| form.surface.vertices.clone());
    let accepted = key(app, revision, tolerance);
    let identity = SelectionIdentity::current(app);
    let live = &mut app.forms_prototype.live;
    match result {
        Ok(computed) => {
            live.profile_context = fixed_vertices.as_ref().map(|_| profile_context);
            live.fixed_vertices = fixed_vertices.unwrap_or_default();
            live.fixed_perimeter = fixed_perimeter;
            live.index = Some(computed.index);
            live.ready = Some((accepted, computed.mesh, computed.report));
            live.source = Some(computed.exact.definition.clone());
            live.exact = Some((accepted, Arc::new(computed.exact)));
            live.accepted_identity = identity;
            live.requested = Some(accepted);
            live.error = None;
            if placed {
                live.placement_reuses = live.placement_reuses.saturating_add(1);
            }
        }
        Err(error) => {
            live.error = Some(error);
            live.ready = None;
            live.exact = None;
            live.index = None;
        }
    }
    restore_fixed_profile_handles(app);
    app.forms_prototype.uploaded = 0;
}
pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.forms_prototype.live.queued = false;
    }
    // Interior edits rebuild the lightweight preview. Its virtual UV-box
    // boundary must not replace previously accepted exact CAD rim handles.
    restore_fixed_profile_handles(app);
    let Some(form) = &app.session.forms_prototype else {
        return;
    };
    if form.repair.is_some() && !form.has_tool_preview() {
        app.forms_prototype.live = State { error: form.repair.clone(), ..Default::default() };
        return;
    }
    let Some(tolerance) = controls::tolerance(app) else {
        return;
    };
    if !tolerance.is_finite() || !(1e-6..=1.0).contains(&tolerance) {
        return;
    }
    let requested = key(app, form.revision, tolerance);
    let identity = SelectionIdentity::current(app);
    let interacting = app.forms_prototype.drag.is_some()
        || app.forms_prototype.gizmo_drag.is_some()
        || extrusion::active(app)
        || transform_job::active(app)
        || ctx.input(|i| i.pointer.primary_down());
    let live = &mut app.forms_prototype.live;
    let same_editor = live.accepted_identity.as_ref().zip(identity.as_ref()).is_some_and(|(old, current)| old.same_draft(current));
    if live.ready.is_some() && live.accepted_identity.as_ref() != identity.as_ref() && live.requested == Some(requested) {
        live.requested = None;
    }
    if same_editor
        && let Some(source) = &mut live.source
        && let Some((previous, mesh, _)) = &mut live.ready
        && *previous != requested
        && previous.1 == requested.1
        && same_shape(form, source)
    {
        let relocated = live.exact.as_ref().map(|(_, exact)| exact.relocated(&form.placement)).transpose();
        let relocated = match relocated {
            Ok(relocated) => relocated,
            Err(error) => {
                // Never publish a moved mesh beside an unmoved exact body.
                live.error = Some(error);
                live.ready = None;
                live.exact = None;
                live.index = None;
                live.requested = Some(requested);
                return;
            }
        };
        place_mesh(Arc::make_mut(mesh), &source.placement, &form.placement);
        if let Some(exact) = relocated {
            *source = exact.definition.clone();
            live.exact = Some((requested, Arc::new(exact)));
        } else {
            Arc::make_mut(source).placement = form.placement.clone();
        }
        *previous = requested;
        live.index = Some(Arc::new(solidcraft_engine::geom::MeshIndex::new(mesh)));
        live.accepted_identity = identity.clone();
        live.requested = Some(requested);
        live.placement_reuses = live.placement_reuses.saturating_add(1);
        app.forms_prototype.uploaded = 0;
    }
    if live.requested != Some(requested) {
        live.error = None;
    }
    if let Some(pending) = &live.pending {
        let result = match pending.done.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("CAD preview computation stopped".into())),
        };
        if let Some(result) = result {
            let same_draft = identity.as_ref().is_some_and(|current| pending.identity.same_draft(current));
            let reusable = same_draft && pending.key.1 == requested.1 && same_shape(form, &pending.source);
            let associated = form.matching.is_some() || form.profile.is_some();
            let exact =
                pending.key == requested && identity.as_ref() == Some(&pending.identity) && (!associated || form.definition() == pending.source);
            let associated_while_dragging = associated && interacting;
            let source = live.pending.take().map(|p| p.source);
            if (exact || reusable)
                && !associated_while_dragging
                && let Some(source) = source
            {
                accept(app, tolerance, source, result);
            } else {
                app.forms_prototype.live.requested = None;
            }
        }
    }
    let Some(form) = &app.session.forms_prototype else { return };
    let key = key(app, form.revision, tolerance);
    let live = &mut app.forms_prototype.live;
    // Native fits run on a worker. Browser fits still run synchronously, so never start
    // one from a pointer gesture there until a browser worker is available.
    let wait_for_release = interacting && (form.matching.is_some() || app.preferences.forms_mesh_while_dragging || cfg!(target_arch = "wasm32"));
    if live.pending.is_some() || live.requested == Some(key) || wait_for_release || app.forms_prototype.pending.is_some() {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    if identity.is_none() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let Some(identity) = identity else { return };
    #[cfg(not(target_arch = "wasm32"))]
    let permit = {
        #[cfg(test)]
        let gate = live.worker_gate.clone();
        #[cfg(not(test))]
        let gate = preview_worker_gate();
        let Some(permit) = PreviewPermit::acquire(gate, ctx.clone()) else {
            live.queued = true;
            // A previous editor/window may own the worker's wake context.
            // Retry from THIS window, reading its latest source only then.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            return;
        };
        permit
    };
    // Acquire the process-wide native slot before copying either the Form or
    // its associated document/support context. A busy slot retains no queued
    // Definition and does not mark this source revision as already requested.
    let definition = form.definition();
    let source = definition.clone();
    let context = (definition.profile.is_some() || (definition.matching.is_some() && !form.has_tool_preview())).then(|| Source {
        document: app.session.doc.clone(),
        before: form.editing_feature.map_or_else(|| app.session.model.state(), |id| app.session.model.state_before(id)),
        moves: app.session.pending_moves.clone(),
    });
    live.requested = Some(key);
    live.error = None;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        match std::thread::Builder::new().name("form-cad-preview".into()).spawn(move || {
            let _permit = permit;
            let result = compute_context(definition, tolerance, context);
            let _ = tx.send(result);
        }) {
            Ok(_) => live.pending = Some(Pending { key, source, identity, done: rx }),
            Err(e) => live.error = Some(e.to_string()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    accept(app, tolerance, source, compute_context(definition, tolerance, context));
}
#[cfg(test)]
pub(super) fn install_occlusion_for_test(app: &mut SolveApp, mesh: Mesh) {
    install_for_test(app);
    let (mesh, index) = indexed_mesh(mesh).unwrap();
    app.forms_prototype.live.ready.as_mut().unwrap().1 = mesh;
    app.forms_prototype.live.index = Some(index);
}
#[cfg(test)]
pub(super) fn install_for_test(app: &mut SolveApp) {
    try_install_for_test(app).unwrap();
}
#[cfg(test)]
pub(super) fn try_install_for_test(app: &mut SolveApp) -> Result<(), String> {
    let form = app.session.forms_prototype.as_ref().ok_or("no Form fixture is active")?;
    let definition = form.definition();
    let tolerance = controls::tolerance(app).ok_or("invalid Form fixture tolerance")?;
    let context = (definition.profile.is_some() || definition.matching.is_some()).then(|| Source {
        document: app.session.doc.clone(),
        before: form.editing_feature.map_or_else(|| app.session.model.state(), |id| app.session.model.state_before(id)),
        moves: app.session.pending_moves.clone(),
    });
    let computed = compute_context(definition.clone(), tolerance, context)?;
    let diagnostic_source = solidcraft_engine::forms_prototype::validation::Input::capture(&app.session).map_err(|error| error.to_string())?;
    app.forms_prototype.live = State::default();
    accept(app, tolerance, definition, Ok(computed));
    if let Some(error) = &app.forms_prototype.live.error {
        return Err(error.clone());
    }
    diagnostics::install_source(app, Arc::new(diagnostic_source));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::geom::tess::from_kernel;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_face_cache_reuses_the_accepted_scene_and_isolates_late_preparation() {
        use super::super::face_pick::Answer;
        use solidcraft_engine::doc::kernel;

        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[1,1,1],"levels":0})).unwrap();
        super::super::opened(&mut app);
        install_for_test(&mut app);
        context::tick(&mut app, &egui::Context::default());
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        let ray = |prepared: &super::super::face_pick::Prepared| prepared.front(Vec3::new(0.3, 0.4, 20.0), -Vec3::Z, [0.0, 100.0]).unwrap();

        let first = face_press::query_for_test(&mut app).unwrap();
        let first_scene = first.prepare().unwrap();
        let again = face_press::query_for_test(&mut app).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(std::ptr::eq(first_scene, again.prepare().unwrap()));
        assert!(matches!(ray(first_scene), Answer::Hit { .. }));

        let body = kernel::box_solid(Vec3::new(-2.0, -2.0, 2.0), Vec3::new(2.0, 2.0, 4.0)).unwrap();
        let mesh = Arc::new(body.mesh(&kernel::MeshTolerance::chord(0.05)).unwrap());
        app.forms_prototype.context.entries.push(context::Entry {
            name: Some("opaque fixture".into()),
            index: Some(Arc::new(solidcraft_engine::geom::MeshIndex::new(&mesh))),
            mesh,
            color: None,
            exact: Some(body),
            detached_companion: false,
        });
        // Keep the context version fixed to exercise its actual retained records.
        let blocked = face_press::query_for_test(&mut app).unwrap();
        assert!(!Arc::ptr_eq(&first, &blocked));
        assert!(!first.is_current(&app));
        app.forms_prototype.context.entries.clear();
        install_for_test(&mut app);
        let current = face_press::query_for_test(&mut app).unwrap();
        assert!(!Arc::ptr_eq(&blocked, &current));
        assert!(!Arc::ptr_eq(&first, &current), "a new accepted CAD source gets its own scene even for identical geometry");
        assert!(!blocked.is_current(&app));
        assert!(matches!(ray(blocked.prepare().unwrap()), Answer::Occluded { .. }));
        let current_scene = current.prepare().unwrap();
        assert_eq!(ray(current_scene), ray(first_scene));
        assert!(std::ptr::eq(current_scene, face_press::query_for_test(&mut app).unwrap().prepare().unwrap()));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
    }

    #[test]
    fn accepted_native_face_query_qualifies_all_eight_primitive_families() {
        use super::super::face_pick::{Answer, CaptureError, Owner, Query};
        use solidcraft_engine::{doc::forms::Definition, geom::brep::eval::SurfaceEval, geom::subdivision::primitives::Parameters};

        for kind in ["box", "plane", "quadball", "sphere", "cylinder", "torus", "tube", "pipe"] {
            let parameters = Parameters { kind: kind.into(), dimensions: [10.0; 3], segments: [4, 4, 2], radii: [5.0, 2.0], ..Default::default() };
            let definition = Definition {
                cage: parameters.build().unwrap(),
                parameters,
                placement: Default::default(),
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
                symmetry: None,
                hierarchy: None,
            };
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.forms_prototype = Some(solidcraft_engine::forms_prototype::Form::from_definition(definition, 1, None).unwrap());
            super::super::opened(&mut app);
            assert_eq!(controls::tolerance(&app), Some(app.preferences.forms_cad_tolerance));
            assert!(matches!(Query::capture(&app), Err(CaptureError::FormNotReady)));
            try_install_for_test(&mut app).unwrap_or_else(|error| panic!("{kind} CAD fit: {error}"));
            context::tick(&mut app, &egui::Context::default());
            let before = app.session.forms_prototype.as_ref().unwrap().inspect();
            let document = app.session.doc.clone();
            let query = Query::capture(&app).unwrap();
            assert!(query.is_current(&app));
            let prepared = query.prepare().unwrap();
            let exact = selection_geometry(&app).unwrap();
            let faces = &exact.body.record().faces;
            let mut found = 0;
            for (native, face) in faces.iter().enumerate().step_by(faces.len().div_ceil(16).max(1)) {
                let [[u0, u1], [v0, v1]] = face.domain;
                let chart = SurfaceEval::new(face.parameters()).unwrap();
                let Some(p) = chart.partials(u0 + 0.37 * (u1 - u0), v0 + 0.63 * (v1 - v0)) else { continue };
                let Some(normal) = p[1][0].cross(p[0][1]).normalized() else { continue };
                // A local ray checks native chart ownership independently of
                // viewport projection and excludes distant unrelated layers.
                let answer = prepared.front(p[0][0] + normal, -normal, [0.95, 1.05]).unwrap();
                match answer {
                    Answer::Hit { owner, depth } => {
                        let ExactFaceOwner::Face(logical) = exact.faces[native] else { panic!("{kind} lost native ownership") };
                        assert_eq!(owner, Owner::Face(logical), "{kind} native face {native}");
                        assert!(depth[0] <= 1.0 + 1e-10 && depth[1] >= 1.0 - 1e-10, "{kind} native depth {depth:?}");
                        found += 1;
                    }
                    Answer::Miss | Answer::Unresolved => {}
                    other => panic!("{kind} unexpected ownership {other:?}"),
                }
            }
            assert!(found > 0, "{kind} has no qualified native face ray");
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
            assert!(Arc::ptr_eq(&app.session.doc, &document));
            assert!(app.forms_prototype.elements.is_empty());
            app.session.uid = app.session.uid.wrapping_add(1);
            assert!(!query.is_current(&app));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn live_preview_capacity_survives_receiver_drop_and_coalesces_the_replacement_editors_latest_source() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let gate = Arc::new(AtomicBool::new(false));
        app.forms_prototype.live.worker_gate = gate.clone();
        let old_window = egui::Context::default();
        let permit = PreviewPermit::acquire(gate.clone(), old_window).unwrap();
        let old_form = app.session.forms_prototype.as_ref().unwrap();
        let old_tolerance = controls::tolerance(&app).unwrap();
        let old_key = key(&app, old_form.revision, old_tolerance);
        let old_source = old_form.definition();
        let old_identity = SelectionIdentity::current(&app).unwrap();
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release_tx, release) = std::sync::mpsc::channel();
        let (done_tx, done) = std::sync::mpsc::channel::<WorkerOutcome>();
        let old = std::thread::spawn(move || {
            let _permit = permit;
            entered_tx.send(()).unwrap();
            release.recv().unwrap();
            assert!(done_tx.send(Err("discarded old CAD preview".to_owned())).is_err(), "the closed editor must have dropped its receiver");
        });
        entered.recv().unwrap();
        app.forms_prototype.live.requested = Some(old_key);
        app.forms_prototype.live.pending = Some(Pending { key: old_key, source: old_source, identity: old_identity, done });

        let current_window = egui::Context::default();
        for _ in 0..8 {
            let _ = current_window.run_ui(Default::default(), |_| {});
            if !current_window.has_requested_repaint() {
                break;
            }
        }
        assert!(!current_window.has_requested_repaint());
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = wakes.clone();
        current_window.set_request_repaint_callback(move |_| {
            wake_count.fetch_add(1, Ordering::Relaxed);
        });
        for offset in 1..=3 {
            app.session.execute("form.prototype.close", &json!({})).unwrap();
            app.forms_prototype = Default::default();
            // Production's gate lives outside every editor. Reuse this explicit
            // instance gate across test resets without coupling parallel tests.
            app.forms_prototype.live.worker_gate = gate.clone();
            app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
            tick(&mut app, &current_window);
            assert!(app.forms_prototype.live.pending.is_none());
            assert_eq!(app.forms_prototype.live.requested, None, "a denied slot must not mark a source as already scheduled");
            assert_eq!(inspect(&app)["queued"], true);
            assert!(busy(&app));
            assert!(gate.load(Ordering::Acquire));
            let point = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::Z * f64::from(offset);
            app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y,point.z]]})).unwrap();
            tick(&mut app, &current_window);
            assert!(app.forms_prototype.live.pending.is_none());
            assert_eq!(app.forms_prototype.live.requested, None);
            assert!(app.forms_prototype.live.source.is_none(), "queued work must not retain a copied Definition");
        }
        assert!(
            wakes.load(Ordering::Relaxed) > 0,
            "the waiting current window must schedule its own retry even when the old worker belongs to another context"
        );
        app.forms_prototype.controls.tolerance = "0.02 mm".into();
        tick(&mut app, &current_window);
        let expected = app.session.forms_prototype.as_ref().unwrap().definition();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        let document = app.session.doc.clone();
        assert!(gate.load(Ordering::Acquire));
        assert_eq!(app.forms_prototype.live.requested, None);
        release_tx.send(()).unwrap();
        old.join().unwrap();
        assert!(!gate.load(Ordering::Acquire), "dropping receivers must not release the slot before the actual old worker exits");
        tick(&mut app, &current_window);
        assert_eq!(inspect(&app)["queued"], false);
        let next = app.forms_prototype.live.pending.take().unwrap();
        assert_eq!(next.source, expected, "only the latest edited source may be copied when the slot becomes available");
        assert_eq!(next.key, key(&app, app.session.forms_prototype.as_ref().unwrap().revision, 0.02));
        let result = next.done.recv().unwrap();
        assert!(result.is_ok());
        let (sender, done) = std::sync::mpsc::channel();
        sender.send(result).unwrap();
        app.forms_prototype.live.pending = Some(Pending { done, ..next });
        tick(&mut app, &current_window);
        assert!(app.forms_prototype.live.pending.is_none());
        assert!(mesh(&app).is_some());
        assert_eq!(app.forms_prototype.live.source.as_ref().unwrap().as_ref(), &expected);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        assert!(Arc::ptr_eq(&app.session.doc, &document));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn live_preview_permit_releases_and_wakes_when_a_worker_unwinds_or_its_spawn_closure_is_dropped() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let gate = Arc::new(AtomicBool::new(false));
        let ctx = egui::Context::default();
        let settle = || {
            for _ in 0..8 {
                let _ = ctx.run_ui(Default::default(), |_| {});
                if !ctx.has_requested_repaint() {
                    break;
                }
            }
            assert!(!ctx.has_requested_repaint(), "each permit release must wake an idle context");
        };
        settle();
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = wakes.clone();
        ctx.set_request_repaint_callback(move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        let permit = PreviewPermit::acquire(gate.clone(), ctx.clone()).unwrap();
        let failed = std::thread::spawn(move || {
            let _permit = permit;
            panic!("deliberate preview worker unwind");
        });
        assert!(failed.join().is_err());
        assert!(!gate.load(Ordering::Acquire));
        assert!(wakes.load(Ordering::Relaxed) > 0);
        assert!(ctx.has_requested_repaint());
        // egui only invokes this callback for a shorter repaint delay. Drain
        // the unwind's request before testing the rejected-spawn wake itself.
        settle();
        let before = wakes.load(Ordering::Relaxed);
        let permit = PreviewPermit::acquire(gate.clone(), ctx.clone()).unwrap();
        let rejected_spawn = move || {
            let _permit = permit;
        };
        assert!(PreviewPermit::acquire(gate.clone(), ctx.clone()).is_none());
        drop(rejected_spawn);
        assert!(wakes.load(Ordering::Relaxed) > before);
        assert!(ctx.has_requested_repaint());
        assert!(PreviewPermit::acquire(gate, ctx).is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn check_cad_worker_keeps_its_permit_across_editor_cancellation_and_replacement() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        install_for_test(&mut app);
        let gate = Arc::new(AtomicBool::new(false));
        let starts = Arc::new(AtomicUsize::new(0));
        let checks = Arc::new(AtomicUsize::new(0));
        let checked = checks.clone();
        app.session.set_command_check(Some(Arc::new(move |id, _, _| {
            if id == "form.prototype.validate" {
                checked.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        })));
        let (entered, enter) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let started = starts.clone();
        let old = diagnostics::start_controlled_for_test(&mut app, &egui::Context::default(), gate.clone(), move || {
            started.fetch_add(1, Ordering::Relaxed);
            entered.send(()).unwrap();
            released.recv().unwrap();
        })
        .unwrap();
        enter.recv().unwrap();
        assert!(gate.load(Ordering::Acquire));
        for _ in 0..3 {
            app.session.execute("form.prototype.close", &json!({})).unwrap();
            // Cancel/close replaces the UI editor and drops its old receiver.
            app.forms_prototype = Default::default();
            app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
            install_for_test(&mut app);
            let started = starts.clone();
            assert!(
                diagnostics::start_controlled_for_test(&mut app, &egui::Context::default(), gate.clone(), move || {
                    started.fetch_add(1, Ordering::Relaxed);
                })
                .is_none()
            );
            assert!(gate.load(Ordering::Acquire));
            assert_eq!(diagnostics::inspect(&app)["pending"], false);
        }
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        assert_eq!(checks.load(Ordering::Relaxed), 1, "a disabled busy action must not authorize or launch another check");
        release.send(()).unwrap();
        old.join().unwrap();
        assert!(!gate.load(Ordering::Acquire));
        diagnostics::tick_for_test(&mut app);
        assert!(diagnostics::current_for_test(&app).is_none(), "the cancelled editor's result must not enter its replacement");
        let started = starts.clone();
        diagnostics::start_controlled_for_test(&mut app, &egui::Context::default(), gate.clone(), move || {
            started.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap()
        .join()
        .unwrap();
        assert!(!gate.load(Ordering::Acquire));
        diagnostics::tick_for_test(&mut app);
        assert!(diagnostics::current_for_test(&app).unwrap().is_ok());
        assert_eq!(starts.load(Ordering::Relaxed), 2);
        assert_eq!(checks.load(Ordering::Relaxed), 2);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn check_cad_worker_unwinding_releases_the_permit_and_reports_disconnection() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        install_for_test(&mut app);
        let gate = Arc::new(AtomicBool::new(false));
        let failed = diagnostics::start_controlled_for_test(&mut app, &egui::Context::default(), gate.clone(), || {
            panic!("deliberate test worker unwind");
        })
        .unwrap();
        assert!(failed.join().is_err());
        assert!(!gate.load(Ordering::Acquire));
        diagnostics::tick_for_test(&mut app);
        assert!(diagnostics::current_for_test(&app).unwrap().is_err());
        diagnostics::start_controlled_for_test(&mut app, &egui::Context::default(), gate.clone(), || {}).unwrap().join().unwrap();
        assert!(!gate.load(Ordering::Acquire));
        diagnostics::tick_for_test(&mut app);
        assert!(diagnostics::current_for_test(&app).unwrap().is_ok());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn check_cad_button_honours_the_live_scoped_host_check_once_for_refusal_panic_and_allow() {
        use solidcraft_engine::schema::ParamCheck;
        use std::sync::atomic::{AtomicUsize, Ordering};
        for outcome in 0..3 {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
            install_for_test(&mut app);
            app.session.param_check = ParamCheck::Strict;
            let source = app.session.forms_prototype.as_ref().unwrap().source_revision();
            let uid = app.session.uid;
            let scope = solidcraft_engine::io::vfs::PathScope::new([std::env::temp_dir()]);
            let expected_scope = scope.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let called = calls.clone();
            app.session.set_command_check(Some(Arc::new(move |id, params, session| {
                called.fetch_add(1, Ordering::Relaxed);
                assert_eq!(id, "form.prototype.validate");
                assert_eq!(params, &json!({"tolerance":0.01}));
                assert_eq!(session.uid, uid);
                assert_eq!(session.forms_prototype.as_ref().unwrap().source_revision(), source);
                assert_eq!(session.param_check, ParamCheck::Strict);
                assert_eq!(solidcraft_engine::io::vfs::current_scope(), Some(expected_scope.clone()));
                if outcome == 1 {
                    panic!("deliberate Check CAD button host panic");
                }
                if outcome == 0 { Err("host refused the Check CAD button".into()) } else { Ok(()) }
            })));
            let before = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
            solidcraft_engine::io::vfs::scoped(Some(scope), || diagnostics::start_for_test(&mut app, &egui::Context::default()));
            assert!(solidcraft_engine::io::vfs::current_scope().is_none());
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            if outcome < 2 {
                assert_eq!(diagnostics::inspect(&app)["pending"], false);
                let error = diagnostics::current_for_test(&app).unwrap().as_ref().unwrap_err();
                assert!(error.contains(if outcome == 0 { "host refused" } else { "host's command check failed" }));
            } else {
                diagnostics::flush_check_for_test(&mut app).unwrap();
                assert!(diagnostics::current_for_test(&app).unwrap().is_ok());
                assert_eq!(calls.load(Ordering::Relaxed), 1, "the worker must not ask the live host check again");
            }
            assert_eq!(app.session.param_check, ParamCheck::Strict);
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), before);
        }
    }
    #[test]
    fn check_cad_worker_preserves_command_schema_levels_and_paired_audit_signatures() {
        use solidcraft_engine::schema::ParamCheck;
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let input = Arc::new(solidcraft_engine::forms_prototype::validation::Input::capture(&app.session).unwrap());
        let before = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        for level in ParamCheck::ALL {
            app.session.param_check = level;
            for params in [
                json!({"tolerance":0.01}),
                json!({"tolerance":null}),
                json!({"tolerance":"wrong type"}),
                json!({"tolerance":0.01,"ui_check_cad_audit_probe":true}),
            ] {
                let command = app.session.execute("form.prototype.validate", &params).map_err(|error| error.to_string());
                let worker = diagnostics::compute_value_for_test(input.clone(), &params, level).map_err(|error| error.to_string());
                assert_eq!(worker, command, "UI query must keep command checking semantics at {level:?}: {params}");
                assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), before);
            }
        }
        // When the existing external param-audit recorder is enabled, paired
        // command/query calls must aggregate as the same audit signatures. Do
        // not mutate the process-wide audit environment from a parallel test.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(dir) = std::env::var_os("SOLIDCRAFT_PARAM_AUDIT") {
            let path = std::path::PathBuf::from(dir).join(format!("audit-{}.jsonl", std::process::id()));
            if let Ok(text) = std::fs::read_to_string(path) {
                let own = std::thread::current().name().unwrap_or("unnamed").to_owned();
                let records: Vec<serde_json::Value> = text
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .filter(|line: &serde_json::Value| {
                        line["command"] == "form.prototype.validate"
                            && line["source"] == own
                            && line["sample_params"].as_str().is_some_and(|sample| sample.contains("ui_check_cad_audit_probe"))
                    })
                    .collect();
                if !records.is_empty() {
                    assert_eq!(records.iter().filter(|line| line["outcome"] == "refused").map(|line| line["count"].as_u64().unwrap()).max(), Some(2));
                    assert_eq!(
                        records
                            .iter()
                            .filter(|line| line["outcome"].as_str().is_some_and(|outcome| outcome.starts_with("error:")))
                            .map(|line| line["count"].as_u64().unwrap())
                            .max(),
                        Some(4)
                    );
                }
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn diagnostic_source_producer_waits_for_pointer_release_instead_of_copying_each_sculpt_frame() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let ctx = egui::Context::default();
        let event = |pressed| egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: Pos2::new(30.0, 30.0),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run_ui(event(true), |_| diagnostics::prepare(&mut app, &ctx));
        assert!(diagnostics::source_for_test(&app).is_none());
        let _ = ctx.run_ui(event(false), |_| diagnostics::prepare(&mut app, &ctx));
        diagnostics::flush_source_for_test(&mut app, &ctx).unwrap();
        assert!(diagnostics::source_for_test(&app).is_some());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn read_query_preparation_is_independent_of_invalid_gui_tolerance_and_tool_apply_flags() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        app.forms_prototype.controls.tolerance = "invalid GUI expression".into();
        let ctx = egui::Context::default();
        diagnostics::prepare(&mut app, &ctx);
        diagnostics::flush_source_for_test(&mut app, &ctx).unwrap();
        let first = diagnostics::source_for_test(&app).unwrap();
        assert!(selection_geometry(&app).is_none(), "query preparation must not require a fitted display");
        let report = first.check(&solidcraft_engine::schema::forms_prototype::Validate { tolerance: Some(0.01) }).unwrap();
        assert_eq!(report.stage, solidcraft_engine::schema::forms_prototype::CheckStage::Complete);
        app.forms_prototype.controls.tolerance = "0.1 mm".into();
        assert!(Arc::ptr_eq(&first, &diagnostics::source_for_test(&app).unwrap()));

        app.session.execute("form.prototype.finish_options", &json!({"operation":"new"})).unwrap();
        assert!(diagnostics::source_for_test(&app).is_none());
        diagnostics::prepare(&mut app, &ctx);
        diagnostics::flush_source_for_test(&mut app, &ctx).unwrap();
        let options = diagnostics::source_for_test(&app).unwrap();
        assert!(Arc::ptr_eq(&first.definition(), &options.definition()), "context/options changes share source geometry");
        app.session.execute("form.prototype.tool.begin", &json!({})).unwrap();
        assert!(diagnostics::source_for_test(&app).is_none());
        diagnostics::prepare(&mut app, &ctx);
        diagnostics::flush_source_for_test(&mut app, &ctx).unwrap();
        assert!(
            diagnostics::source_for_test(&app)
                .unwrap()
                .check(&solidcraft_engine::schema::forms_prototype::Validate { tolerance: Some(0.01) })
                .is_err()
        );
        app.session.execute("form.prototype.tool.apply", &json!({})).unwrap();
        assert!(diagnostics::source_for_test(&app).is_none(), "Apply changes eligibility even without a source revision increment");
        diagnostics::prepare(&mut app, &ctx);
        diagnostics::flush_source_for_test(&mut app, &ctx).unwrap();
        assert!(
            diagnostics::source_for_test(&app)
                .unwrap()
                .check(&solidcraft_engine::schema::forms_prototype::Validate { tolerance: Some(0.01) })
                .is_ok()
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn source_preparation_rejects_a_late_replaced_editor_before_installing_its_input() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let ctx = egui::Context::default();
        diagnostics::prepare(&mut app, &ctx);
        app.session.execute("form.prototype.close", &json!({})).unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        diagnostics::flush_source_for_test(&mut app, &ctx).unwrap();
        assert!(diagnostics::source_for_test(&app).is_none(), "old same-looking source cannot become the replacement's query");
        diagnostics::flush_source_for_test(&mut app, &ctx).unwrap();
        assert!(diagnostics::source_for_test(&app).is_some());
    }
    #[test]
    fn diagnostic_job_capture_shares_producer_input_without_editor_history_or_display_buffers() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":0})).unwrap();
        for step in 1..=64 {
            let point = app.session.forms_prototype.as_ref().unwrap().cage.points[0];
            app.session
                .execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y+f64::from(step)*0.0001,point.z]]}))
                .unwrap();
        }
        install_for_test(&mut app);
        let input = diagnostics::source_for_test(&app).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 64);
        // Display storage is not part of a source query. Growing it cannot
        // replace or copy the cached query captured by a button/agent worker.
        let surface = std::sync::Arc::make_mut(&mut app.session.forms_prototype.as_mut().unwrap().surface);
        surface.mesh.positions.extend(std::iter::repeat_n(Vec3::ZERO, 100_000));
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        let again = diagnostics::source_for_test(&app).unwrap();
        assert!(Arc::ptr_eq(&input, &again));
        let report = input.check(&solidcraft_engine::schema::forms_prototype::Validate { tolerance: Some(0.01) }).unwrap();
        assert_eq!(report.stage, solidcraft_engine::schema::forms_prototype::CheckStage::Complete);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert!(diagnostics::source_for_test(&app).is_none());
    }
    #[test]
    fn failed_fit_retains_its_current_read_query_without_accepting_a_display_body() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let requested = key(&app, form.revision, 0.01);
        let source = form.definition();
        let input = Arc::new(solidcraft_engine::forms_prototype::validation::Input::capture(&app.session).unwrap());
        diagnostics::install_source(&mut app, input.clone());
        let (sender, done) = std::sync::mpsc::channel();
        sender.send(Err("conversion refused for this preview".to_owned())).unwrap();
        app.forms_prototype.live.requested = Some(requested);
        app.forms_prototype.live.pending = Some(Pending { key: requested, source, identity: SelectionIdentity::current(&app).unwrap(), done });
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| tick(&mut app, &ctx));
        assert!(app.forms_prototype.live.error.is_some());
        assert!(selection_geometry(&app).is_none());
        assert!(Arc::ptr_eq(&input, &diagnostics::source_for_test(&app).unwrap()));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        app.session.uid = app.session.uid.wrapping_add(1);
        assert!(diagnostics::source_for_test(&app).is_none());
    }
    #[test]
    fn accepted_selection_and_occlusion_snapshots_share_worker_buffers_and_survive_editor_reset() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[2,2,2],"levels":1})).unwrap();
        install_for_test(&mut app);
        let exact = selection_geometry(&app).unwrap().clone();
        let logical = exact.logical.as_ref().unwrap().clone();
        let (mesh, index) = occlusion_snapshot(&app).unwrap();
        let (again_mesh, again_index) = occlusion_snapshot(&app).unwrap();
        assert!(Arc::ptr_eq(&exact, selection_geometry(&app).unwrap()));
        assert!(Arc::ptr_eq(&exact.definition, app.forms_prototype.live.source.as_ref().unwrap()));
        assert!(Arc::ptr_eq(&logical, selection_geometry(&app).unwrap().logical.as_ref().unwrap()));
        assert!(Arc::ptr_eq(&mesh, &again_mesh));
        assert!(Arc::ptr_eq(&index, &again_index));
        let face_count = exact.body.face_count();
        let point = mesh.positions[0];
        app.forms_prototype = Default::default();
        assert!(selection_geometry(&app).is_none());
        assert_eq!(exact.body.face_count(), face_count);
        assert_eq!(mesh.positions[0], point);
        assert!(logical.accepts(&BTreeSet::from([Element::Face(0)]), Pick::Face));
        assert!(!logical.accepts(&BTreeSet::from([Element::Face(usize::MAX)]), Pick::Face));
    }
    #[test]
    fn identical_replacement_draft_and_wrong_document_cannot_reuse_an_accepted_source_stamp() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        install_for_test(&mut app);
        let before = app.session.forms_prototype.as_ref().unwrap().source_revision();
        app.session.uid = app.session.uid.wrapping_add(1);
        assert!(selection_geometry(&app).is_none());
        assert!(diagnostics::source_for_test(&app).is_none());
        app.session.uid = app.session.uid.wrapping_sub(1);
        assert!(selection_geometry(&app).is_some());
        app.session.execute("form.prototype.close", &json!({})).unwrap();
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        let after = app.session.forms_prototype.as_ref().unwrap().source_revision();
        assert_eq!(before.1, after.1);
        assert_ne!(before.0, after.0);
        assert!(selection_geometry(&app).is_none());
        assert!(diagnostics::source_for_test(&app).is_none());
    }
    #[test]
    fn display_index_preflight_short_circuits_overlong_paths_before_later_buffers() {
        let examined = std::cell::Cell::new(0);
        let lengths = [MAX_DISPLAY_POINTS, 1, usize::MAX].into_iter().inspect(|_| examined.set(examined.get() + 1));
        assert!(display_path_count(lengths).is_err());
        assert_eq!(examined.get(), 2);
        assert!(display_path_count([usize::MAX].into_iter()).is_err());
        assert!(validate_display_mesh(&Mesh::default()).is_ok());
    }
    #[test]
    fn cached_profile_point_ids_include_only_current_resolved_fixed_handles() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.cylinder", &json!({"radius":3,"height":2,"body_name":"Support"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&id| body.planar_face(id).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0})).unwrap();
        install_for_test(&mut app);
        let form = app.session.forms_prototype.as_ref().unwrap();
        let boundary = form.profile.as_ref().unwrap().boundary();
        let logical = selection_geometry(&app).unwrap().logical.as_ref().unwrap();
        for id in 0..form.logical.points.len() {
            assert_eq!(logical.allowed.contains(&Element::Vertex(id)), form.selectable_control(id) && handle_available(&app, id));
        }
        assert!(boundary.iter().all(|id| logical.allowed.contains(&Element::Vertex(*id))));
        app.forms_prototype.live.fixed_vertices.clear();
        assert!(selection_geometry(&app).is_some(), "the source is current even while its derived handles are unavailable");
        let form = app.session.forms_prototype.as_ref().unwrap();
        let allowed = selection_allowed(&app).unwrap();
        for id in 0..form.logical.points.len() {
            assert_eq!(allowed.contains(&Element::Vertex(id)), form.selectable_control(id) && handle_available(&app, id));
        }
        assert!(boundary.iter().all(|id| !allowed.contains(&Element::Vertex(*id))));
        install_for_test(&mut app);
        app.session.doc_mut().features[0].name.push_str(" changed support context");
        assert!(selection_geometry(&app).is_none());
        assert!(boundary.iter().all(|id| !handle_available(&app, *id)));
        install_for_test(&mut app);
        assert!(boundary.iter().all(|id| {
            selection_geometry(&app).unwrap().logical.as_ref().unwrap().allowed.contains(&Element::Vertex(*id)) && handle_available(&app, *id)
        }));
    }
    #[test]
    fn all_primitive_routes_have_world_rational_controls_and_exact_logical_endpoints() {
        use solidcraft_engine::doc::forms::{Definition, SelectionGuides};
        use solidcraft_engine::geom::subdivision::primitives::Parameters;
        for kind in ["box", "plane", "quadball", "sphere", "cylinder", "torus", "tube", "pipe"] {
            let parameters = Parameters { kind: kind.into(), dimensions: [10.0; 3], segments: [4, 4, 2], radii: [5.0, 2.0], ..Default::default() };
            let cage = parameters.build().unwrap();
            let edges = cage.edges().unwrap();
            let definition = Definition {
                cage,
                parameters,
                placement: Default::default(),
                imported: None,
                pipe: None,
                profile: None,
                matching: None,
                symmetry: None,
                hierarchy: None,
            };
            let computed = compute_with_body(definition.clone(), 0.05).unwrap();
            let SelectionGuides::Subdivision(curves) = &computed.exact.guides else { panic!("{kind}: {:?}", computed.exact.guides) };
            assert_eq!(curves.iter().map(|curve| curve.edge).collect::<BTreeSet<_>>(), edges.into_iter().collect());
            let points = computed.exact.points.as_ref().unwrap();
            for curve in curves {
                assert!(points[&curve.edge[0]].dist(curve.at(0.0).unwrap()) <= 1e-7, "{kind}");
                assert!(points[&curve.edge[1]].dist(curve.at(1.0).unwrap()) <= 1e-7, "{kind}");
                assert!(curve.rational_pieces_bounded(&mut 0).is_err());
                for (range, rational) in curve.rational_pieces_bounded(&mut 1_000_000).unwrap() {
                    assert!(rational.ctrl.iter().all(|control| control[3] > 0.0 && control.iter().all(|value| value.is_finite())));
                    for t in [0.0, 0.37, 1.0] {
                        let (h, _) = rational.eval(t).unwrap();
                        let point = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
                        assert!(point.dist(curve.at(range[0] * (1.0 - t) + range[1] * t).unwrap()) < 1e-7, "{kind}");
                    }
                }
            }
            assert_eq!(*computed.exact.definition, definition);
        }
    }
    #[test]
    fn retained_exact_payload_preserves_base_and_deleted_leaf_provenance() {
        use solidcraft_engine::geom::subdivision::{hierarchy::Hierarchy, primitives::Parameters};
        let parameters = Parameters { dimensions: [10.0; 3], segments: [1; 3], ..Default::default() };
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        let mut definition = app.session.forms_prototype.as_ref().unwrap().definition();
        definition.parameters = parameters.clone();
        definition.cage = parameters.build().unwrap();
        let computed = compute_with_body(definition.clone(), 0.05).unwrap();
        assert_eq!(*computed.exact.definition, definition);
        assert_eq!(computed.exact.body.face_count(), computed.report.patch_roots.len());
        assert_eq!(computed.exact.faces, computed.report.patch_roots.iter().copied().map(ExactFaceOwner::Face).collect::<Vec<_>>());
        let hierarchy = Hierarchy::new(definition.cage.clone()).unwrap().subdivide(&[0], [2, 1]).unwrap().delete_faces(&[6]).unwrap();
        definition.cage = hierarchy.evaluate().unwrap();
        definition.hierarchy = Some(hierarchy);
        let before = definition.clone();
        let computed = compute_with_body(definition, 0.05).unwrap();
        assert!(computed.exact.body.is_surface());
        assert_eq!(*computed.exact.definition, before, "retaining native data must not materialize or reset the hierarchy");
        assert_eq!(computed.exact.faces, computed.report.patch_roots.iter().copied().map(ExactFaceOwner::Face).collect::<Vec<_>>());
        assert!(!computed.exact.faces.contains(&ExactFaceOwner::Face(0)));
        assert!(!computed.exact.faces.contains(&ExactFaceOwner::Face(6)));
        assert!(computed.exact.faces.contains(&ExactFaceOwner::Face(7)));
        let unknown = ExactSelection::new(computed.exact.body.clone(), before.clone(), None);
        assert!(unknown.faces.iter().all(|owner| *owner == ExactFaceOwner::Unmapped));
        let invalid_owners = vec![usize::MAX; computed.exact.body.face_count()];
        let malformed = ExactSelection::new(computed.exact.body.clone(), before, Some(&invalid_owners));
        assert!(malformed.faces.iter().all(|owner| *owner == ExactFaceOwner::Unmapped));
    }
    #[test]
    fn retained_exact_payload_includes_derived_trim_wall_owner_without_virtual_cage_guessing() {
        use solidcraft_engine::geom::subdivision::{hierarchy::Hierarchy, primitives::Parameters};
        let parameters = Parameters { kind: "face".into(), dimensions: [20.0, 20.0, 1.0], segments: [4, 4, 1], ..Default::default() };
        let base = parameters.build().unwrap();
        let deleted = base.faces.len();
        let refined = Hierarchy::new(base).unwrap().subdivide(&[5], [2, 2]).unwrap();
        let ring = refined.layout().unwrap().faces[deleted].clone();
        let trimmed = refined.delete_faces(&[deleted]).unwrap();
        let edge = ring
            .iter()
            .copied()
            .zip(ring.iter().copied().cycle().skip(1))
            .take(ring.len())
            .find_map(|(a, b)| {
                trimmed.trim_attachments(&[[a, b]]).ok()?.into_iter().find(|attachment| attachment.root == 5).map(|attachment| attachment.edge)
            })
            .unwrap();
        let hierarchy = trimmed.extend_edges(&[edge], Vec3::Z * 2.0).unwrap().0;
        let wall_owner = hierarchy.leaves.len();
        let definition = solidcraft_engine::doc::forms::Definition {
            cage: hierarchy.evaluate().unwrap(),
            parameters,
            hierarchy: Some(hierarchy),
            placement: Default::default(),
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            symmetry: None,
        };
        let computed = compute_with_body(definition.clone(), 0.05).unwrap();
        assert_eq!(*computed.exact.definition, definition);
        assert!(computed.exact.body.is_surface());
        assert!(computed.exact.faces.contains(&ExactFaceOwner::Face(wall_owner)));
        let solidcraft_engine::doc::forms::SelectionGuides::Subdivision(curves) = &computed.exact.guides else {
            panic!("{:?}", computed.exact.guides)
        };
        let first = definition.cage.points.len();
        assert!(curves.iter().any(|curve| curve.edge == [first, first + 1]));
        let points = computed.exact.points.as_ref().unwrap();
        for (base, virtual_id) in [(edge[0], first), (edge[1], first + 1)] {
            assert!(points[&virtual_id].dist(points[&base] + Vec3::Z * 2.0) < 1e-7);
        }
        assert!(!computed.exact.faces.contains(&ExactFaceOwner::Face(deleted)));
        assert!(!computed.exact.faces.contains(&ExactFaceOwner::Unmapped));
        assert_eq!(computed.exact.faces, computed.report.patch_roots.iter().copied().map(ExactFaceOwner::Face).collect::<Vec<_>>());
    }
    #[test]
    fn retained_profile_geometry_is_only_the_edited_skin_with_explicit_surface_or_cell_owners() {
        for partition in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"})).unwrap();
            let support = app.session.model.state().bodies[0].body.clone();
            let top = (0..support.face_count()).find(|&face| support.planar_face(face).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
            app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[2,2],"continuity":0,"partition":partition})).unwrap();
            let definition = app.session.forms_prototype.as_ref().unwrap().definition();
            let before = app.session.forms_prototype.as_ref().unwrap().inspect();
            let document = app.session.doc.clone();
            let context = Source { document: document.clone(), before: app.session.model.state(), moves: Default::default() };
            let computed = compute_context(definition.clone(), 0.01, Some(context)).unwrap();
            assert!(computed.exact.body.is_surface(), "the support solid and its unrelated faces must not leak into edited-skin selection");
            assert_eq!(computed.exact.body.face_count(), computed.report.patches);
            let solidcraft_engine::doc::forms::SelectionGuides::Native(routes) = &computed.exact.guides else {
                panic!("{:?}", computed.exact.guides)
            };
            assert_eq!(routes.edges.len(), definition.cage.edges().unwrap().len());
            assert_eq!(computed.exact.points.as_ref().unwrap().len(), definition.cage.points.len());
            assert_eq!(computed.exact.faces.len(), computed.exact.body.face_count());
            if partition {
                assert!(computed.exact.faces.iter().all(|owner| matches!(owner, ExactFaceOwner::Face(0..=3))));
            } else {
                assert!(computed.exact.faces.iter().all(|owner| *owner == ExactFaceOwner::Surface));
                let mut requested_cells = definition.clone();
                requested_cells.profile.as_mut().unwrap().partition = true;
                let fallback = ExactSelection::new(computed.exact.body.clone(), requested_cells, None);
                assert!(
                    fallback.faces.iter().all(|owner| *owner == ExactFaceOwner::Surface),
                    "a requested partition must not invent native cell ownership when the fit falls back"
                );
                assert!(computed.report.patch_roots.iter().all(|&owner| owner == 0), "the compatibility report must not determine Surface identity");
            }
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
            assert!(Arc::ptr_eq(&app.session.doc, &document));
            accept(&mut app, 0.01, definition, Ok(computed));
            let exact = selection_geometry(&app).unwrap();
            assert_eq!(*exact.definition, app.session.forms_prototype.as_ref().unwrap().definition());
            let points = exact.points.as_ref().unwrap();
            let expected = (points[&4] - points[&1]).normalized().unwrap();
            app.forms_prototype.orientation = 2;
            app.forms_prototype.elements = BTreeSet::from([Element::Edge([1, 4])]);
            let axes = super::basis(&app).unwrap();
            assert!(axes[0].dot(expected) > 0.999999);
            assert!(axes[2].z.abs() > 0.999999);
            assert!(axes[0].cross(axes[1]).dist(axes[2]) < 1e-8);
            app.forms_prototype.elements = BTreeSet::from([Element::Vertex(4)]);
            assert!(super::basis(&app).unwrap()[2].z.abs() > 0.999999);
            if !partition {
                app.forms_prototype.elements = BTreeSet::from([Element::Surface]);
                assert_eq!(super::basis(&app), Err("forms.selection_frame.unavailable"));
            }
            app.session.doc_mut().features[0].name.push_str(" revised context");
            assert!(selection_geometry(&app).is_none(), "native geometry must be guarded by source context as well as form revision");
        }
    }
    #[test]
    fn exact_payload_access_rejects_stale_keys_sources_and_failed_results_atomically() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        install_for_test(&mut app);
        let definition = app.session.forms_prototype.as_ref().unwrap().definition();
        let document = app.session.doc.clone();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        assert!(selection_geometry(&app).is_some());
        let mut almost_rigid = definition.placement.clone();
        almost_rigid.axes[0].x += 2e-9;
        assert!(almost_rigid.validate().is_ok());
        assert!(selection_geometry(&app).unwrap().relocated(&almost_rigid).is_err());
        assert_eq!(*selection_geometry(&app).unwrap().definition, definition);
        app.forms_prototype.controls.tolerance = "0.02 mm".into();
        assert!(selection_geometry(&app).is_none());
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        assert!(selection_geometry(&app).is_some());
        app.forms_prototype.live.exact.as_mut().unwrap().0.0 ^= 1;
        assert!(selection_geometry(&app).is_none());
        app.forms_prototype.live.exact.as_mut().unwrap().0.0 ^= 1;
        Arc::make_mut(&mut Arc::make_mut(&mut app.forms_prototype.live.exact.as_mut().unwrap().1).definition).cage.points[0].x += 1.0;
        assert!(selection_geometry(&app).is_none());
        Arc::make_mut(&mut app.forms_prototype.live.exact.as_mut().unwrap().1).definition = app.forms_prototype.live.source.as_ref().unwrap().clone();
        assert!(selection_geometry(&app).is_some());
        accept(&mut app, 0.01, definition, Err("deliberate fit refusal".into()));
        assert!(selection_geometry(&app).is_none());
        assert!(mesh(&app).is_none());
        assert!(app.forms_prototype.live.exact.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        assert!(Arc::ptr_eq(&app.session.doc, &document));
    }
    #[test]
    fn an_old_worker_result_never_installs_exact_geometry_after_a_shape_edit() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let source = form.definition();
        let requested = key(&app, form.revision, 0.01);
        let computed = compute_context(source.clone(), 0.01, None);
        let (sender, receiver) = std::sync::mpsc::channel();
        assert!(sender.send(computed).is_ok());
        app.forms_prototype.live.pending =
            Some(Pending { key: requested, source, identity: SelectionIdentity::current(&app).unwrap(), done: receiver });
        let point = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::X;
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x,point.y,point.z]]})).unwrap();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        let context = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: Pos2::new(30.0, 30.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = context.run_ui(input, |_| tick(&mut app, &context));
        assert!(app.forms_prototype.live.pending.is_none());
        assert!(app.forms_prototype.live.ready.is_none());
        assert!(selection_geometry(&app).is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
    }
    #[test]
    fn pending_profile_pulls_cancel_and_history_retain_only_resolved_fixed_handles() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.cylinder", &json!({"radius":3,"height":2,"body_name":"Support"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let boundary = form.profile.as_ref().unwrap().boundary();
        let interior: Vec<_> = (0..form.cage.points.len()).filter(|i| !boundary.contains(i)).collect();
        assert_eq!(interior.len(), 2);
        assert!(boundary.iter().all(|&id| !handle_available(&app, id)));
        assert!(interior.iter().all(|&id| handle_available(&app, id)));
        let before = form.inspect();
        let definition = form.definition();
        app.cam.fov = 0.0;
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        app.cam.target = Vec3::new(0.0, 0.0, 2.0);
        app.cam.distance = 15.0;
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let scaffold = *boundary.first().unwrap();
        assert_eq!(surface::nearest_with(&app, &projection, projection.to_screen(definition.cage.points[scaffold]).unwrap(), Pick::Vertex), None);
        for &id in &interior {
            assert_eq!(
                surface::nearest_with(&app, &projection, projection.to_screen(definition.cage.points[id]).unwrap(), Pick::Vertex),
                Some(Element::Vertex(id))
            );
        }
        app.forms_prototype.selected.insert(scaffold);
        assert!(triad(&app).is_none());
        app.forms_prototype.selected.clear();
        app.forms_prototype.selected.insert(interior[0]);
        assert!(triad(&app).is_some());
        app.forms_prototype.selected.clear();
        let document = app.session.doc.clone();
        let source = Source { document: document.clone(), before: app.session.model.state(), moves: Default::default() };
        let computed = compute_context(definition.clone(), 0.01, Some(source));
        accept(&mut app, 0.01, definition.clone(), computed);
        assert!(app.forms_prototype.live.error.is_none(), "{:?}", app.forms_prototype.live.error);
        let fixed = app.forms_prototype.live.fixed_vertices.clone();
        assert!(boundary.iter().all(|&id| handle_available(&app, id)));
        let id = interior[0];
        let point = definition.cage.points[id] + Vec3::new(0.12, -0.08, 0.8);
        app.session.execute("form.prototype.set_surface_vertices", &json!({"vertices":[id],"positions":[[point.x,point.y,point.z]]})).unwrap();
        let pulled = app.session.forms_prototype.as_ref().unwrap().inspect();
        let authored = |mut value: serde_json::Value| {
            value.as_object_mut().unwrap().remove("revision");
            value
        };
        assert_eq!(pulled["undo"].as_u64().unwrap(), before["undo"].as_u64().unwrap() + 1);
        assert!(mesh(&app).is_none(), "the accepted CAD belongs to the previous source revision");
        restore_fixed_profile_handles(&mut app);
        for &i in &boundary {
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().surface.vertices[i], fixed[i]);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().surface.mesh.positions[i], fixed[i]);
            assert!(handle_available(&app, i));
        }
        let form = app.session.forms_prototype.as_ref().unwrap();
        for ((edge, curve), cached) in form.surface.boundaries.iter().zip(&app.forms_prototype.live.fixed_perimeter) {
            if let Some(cached) = cached {
                assert_eq!(curve, cached);
                assert!(curve.iter().all(|p| (p.x.hypot(p.y) - 3.0).abs() < 1e-7 && (p.z - 2.0).abs() < 1e-7));
            } else {
                assert_eq!(*curve.first().unwrap(), form.surface.vertices[edge[0]]);
                assert_eq!(*curve.last().unwrap(), form.surface.vertices[edge[1]]);
            }
        }
        assert!(form.surface.mesh.normals.iter().all(|n| n.is_finite() && (n.len() - 1.0).abs() < 1e-10));
        app.cam.fov = 0.0;
        app.cam.pitch = 0.8;
        app.cam.yaw = -0.7;
        app.cam.target = Vec3::new(0.0, 0.0, 2.0);
        app.cam.distance = 18.0;
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1300.0, 1200.0)));
        for &i in &boundary {
            let point = app.session.forms_prototype.as_ref().unwrap().surface.vertices[i];
            assert!(surface::visible(&app, &projection, point), "pending fixed handle {i} must not be occluded by a virtual bbox");
            assert_eq!(surface::nearest_with(&app, &projection, projection.to_screen(point).unwrap(), Pick::Vertex), Some(Element::Vertex(i)));
        }
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().surface.vertices[id], point);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), pulled);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        restore_fixed_profile_handles(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), definition);
        app.session.execute("form.prototype.redo", &json!({})).unwrap();
        restore_fixed_profile_handles(&mut app);
        assert_eq!(authored(app.session.forms_prototype.as_ref().unwrap().inspect()), authored(pulled.clone()));
        app.session.execute("form.prototype.tool.begin", &json!({})).unwrap();
        app.session
            .execute(
                "form.prototype.tool.preview",
                &json!({"surface":{"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false}}),
            )
            .unwrap();
        restore_fixed_profile_handles(&mut app);
        app.session.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        restore_fixed_profile_handles(&mut app);
        assert_eq!(authored(app.session.forms_prototype.as_ref().unwrap().inspect()), authored(pulled.clone()));
        for &i in &boundary {
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().surface.vertices[i], fixed[i]);
        }
        app.session.doc_mut().features[0].name.push_str(" changed context");
        assert!(boundary.iter().all(|&i| !handle_available(&app, i)), "a different support context must never reuse stale CAD handles");
        assert!(interior.iter().all(|&i| handle_available(&app, i)), "editable guides stay available while their CAD fit is pending");
        app.session.doc = document;
        restore_fixed_profile_handles(&mut app);
        assert!(boundary.iter().all(|&i| handle_available(&app, i)));
        assert_eq!(authored(app.session.forms_prototype.as_ref().unwrap().inspect()), authored(pulled.clone()));
    }
    #[test]
    fn circular_profile_fixed_handles_follow_the_cad_rim_without_changing_authored_controls() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.cylinder", &json!({"radius":3,"height":2,"body_name":"Support"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let before = form.inspect();
        let definition = form.definition();
        let boundary = form.profile.as_ref().unwrap().boundary();
        let interior: Vec<_> = (0..form.cage.points.len()).filter(|i| !boundary.contains(i)).collect();
        assert_eq!(interior.len(), 2);
        assert!(boundary.iter().any(|&i| form.surface.vertices[i].x.hypot(form.surface.vertices[i].y) > 3.1));
        let source = Source { document: app.session.doc.clone(), before: app.session.model.state(), moves: Default::default() };
        let computed = compute_context(definition.clone(), 0.01, Some(source));
        accept(&mut app, 0.01, definition.clone(), computed);
        assert!(app.forms_prototype.live.error.is_none(), "{:?}", app.forms_prototype.live.error);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.definition(), definition);
        assert_eq!(form.inspect(), before);
        for &i in &boundary {
            let p = form.surface.vertices[i];
            assert!((p.x.hypot(p.y) - 3.0).abs() < 1e-6, "fixed handle {i} must lie on the actual circular rim");
        }
        for i in interior {
            assert_eq!(form.surface.vertices[i], definition.cage.points[i], "editable guide identity/position must remain authored");
        }
        app.cam.fov = 0.0;
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        app.cam.target = Vec3::new(0.0, 0.0, 2.0);
        app.cam.distance = 15.0;
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        for &i in &boundary {
            let point = app.session.forms_prototype.as_ref().unwrap().surface.vertices[i];
            let screen = projection.to_screen(point).unwrap();
            assert_eq!(surface::nearest_with(&app, &projection, screen, Pick::Vertex), Some(Element::Vertex(i)));
        }
        let form = app.session.forms_prototype.as_ref().unwrap();
        let fixed = *boundary.first().unwrap();
        let position = form.surface.vertices[fixed] + Vec3::Z;
        assert!(
            app.session
                .execute("form.prototype.set_surface_vertices", &json!({"vertices":[fixed],"positions":[[position.x,position.y,position.z]]}))
                .is_ok()
        );
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
    }
    #[test]
    fn release_preview_follows_a_moved_match_support_without_an_extra_undo() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        let support = app.session.execute("solid.box", &json!({"length":4,"width":6,"height":1,"corner":[-2,-3,-1],"body_name":"Support"})).unwrap()
            ["feature"]
            .as_u64()
            .unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":2})).unwrap();
        let cage = &app.session.forms_prototype.as_ref().unwrap().cage;
        let ring = cage.boundary_loops().unwrap().remove(0);
        let edge = [ring[0], ring[1]];
        app.session.execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":edge,"order":1})).unwrap();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].clone();
        if let solidcraft_engine::doc::FeatureKind::Box { height, .. } =
            &mut app.session.doc_mut().features.iter_mut().find(|f| f.id == support).unwrap().kind
        {
            *height = "2 mm".into();
        }
        app.session.refresh();
        let snapshot = app.session.doc.clone();
        let source = Source { document: snapshot.clone(), before: app.session.model.state(), moves: Default::default() };
        let solved = compute_context(before.clone(), 0.01, Some(source));
        accept(&mut app, 0.01, before, solved);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(Arc::ptr_eq(&app.session.doc, &snapshot));
        assert_eq!(form.inspect()["undo"], undo);
        assert!(form.match_report.as_ref().unwrap().position_mm <= 0.001);
        assert!(ring.iter().all(|&v| (form.surface.vertices[v].z - 1.0).abs() < 0.002));
        assert!(app.forms_prototype.live.error.is_none());
    }
    #[test]
    fn release_preview_rebases_an_open_surface_on_a_real_source_edit_without_changing_history() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        let support = app
            .session
            .execute("solid.box", &json!({"corner":[0,0,0],"length":12,"width":8,"height":2,"body_name":"Support","operation":"new"}))
            .unwrap()["feature"]
            .as_u64()
            .unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let boundary = form.profile.as_ref().unwrap().boundary();
        let interior: Vec<_> = (0..form.cage.points.len()).filter(|id| !boundary.contains(id)).collect();
        assert_eq!(interior.len(), 2);
        let targets = [Vec3::new(0.03, -0.04, 0.4), Vec3::new(-0.02, 0.01, 0.15)]
            .into_iter()
            .zip(&interior)
            .map(|(delta, &id)| {
                let point = form.surface.vertices[id] + delta;
                [point.x, point.y, point.z]
            })
            .collect::<Vec<_>>();
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":interior,"positions":targets})).unwrap();
        let second = app.session.forms_prototype.as_ref().unwrap().surface.vertices[interior[0]] + Vec3::Z * 0.1;
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[interior[0]],"positions":[[second.x,second.y,second.z]]})).unwrap();
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let history = (form.inspect()["undo"].clone(), form.inspect()["redo"].clone());
        assert!(history.1.as_u64().unwrap() > 0, "source refresh must preserve an existing Redo branch");
        app.session
            .execute(
                "timeline.redefine",
                &json!({"feature":support,"command":"solid.box","params":{"corner":[0,0,0],"length":12,"width":8,"height":3,"body_name":"Support","operation":"new"}}),
            )
            .unwrap();
        let document = app.session.doc.clone();
        let source = Source { document: document.clone(), before: app.session.model.state(), moves: Default::default() };
        let result = compute_context(before.clone(), 0.01, Some(source));
        accept(&mut app, 0.01, before.clone(), result);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(Arc::ptr_eq(&app.session.doc, &document));
        assert_eq!((form.inspect()["undo"].clone(), form.inspect()["redo"].clone()), history);
        assert!(app.forms_prototype.live.error.is_none());
        assert!(mesh(&app).is_some());
        for (&old, &point) in before.cage.points.iter().zip(&form.cage.points) {
            assert!(point.dist(old + Vec3::Z) < 1e-9);
        }
        for (&old, &point) in before.profile.as_ref().unwrap().rest.iter().zip(&form.profile.as_ref().unwrap().rest) {
            assert!(point.dist(old + Vec3::Z) < 1e-9);
        }
        let accepted = form.definition();
        let revision = form.revision;
        let source = Source { document: document.clone(), before: app.session.model.state(), moves: Default::default() };
        let result = compute_context(accepted.clone(), 0.01, Some(source));
        accept(&mut app, 0.01, accepted.clone(), result);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().revision, revision, "an unchanged source must not repeatedly rewrite the editor");
        app.session.execute("form.prototype.redo", &json!({})).unwrap();
        let redone = app.session.forms_prototype.as_ref().unwrap().definition();
        let source = Source { document: document.clone(), before: app.session.model.state(), moves: Default::default() };
        let result = compute_context(redone.clone(), 0.01, Some(source));
        accept(&mut app, 0.01, redone, result);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(Arc::ptr_eq(&app.session.doc, &document));
        assert!(app.forms_prototype.live.error.is_none());
        assert!(mesh(&app).is_some());
        assert_eq!(form.inspect()["undo"].as_u64().unwrap(), history.0.as_u64().unwrap() + 1);
        assert_eq!(form.inspect()["redo"].as_u64().unwrap() + 1, history.1.as_u64().unwrap());
        for (id, (&old, &point)) in before.cage.points.iter().zip(&form.cage.points).enumerate() {
            let sculpt = if id == interior[0] { Vec3::Z * 0.1 } else { Vec3::ZERO };
            assert!(point.dist(old + Vec3::Z + sculpt) < 1e-9, "Redo must retain the new support height and restore its sculpt");
        }
        for (&old, &point) in before.profile.as_ref().unwrap().rest.iter().zip(&form.profile.as_ref().unwrap().rest) {
            assert!(point.dist(old + Vec3::Z) < 1e-9);
        }
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        let undone = app.session.forms_prototype.as_ref().unwrap().definition();
        let source = Source { document: document.clone(), before: app.session.model.state(), moves: Default::default() };
        let result = compute_context(undone.clone(), 0.01, Some(source));
        accept(&mut app, 0.01, undone, result);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(Arc::ptr_eq(&app.session.doc, &document));
        assert!(app.forms_prototype.live.error.is_none());
        assert!(mesh(&app).is_some());
        assert_eq!((form.inspect()["undo"].clone(), form.inspect()["redo"].clone()), history);
        assert_eq!(form.definition(), accepted, "Undo and source refresh must restore the exact accepted definition");
        app.session.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap();
        let saved = app
            .session
            .doc
            .features
            .iter()
            .find_map(|feature| match &feature.kind {
                solidcraft_engine::doc::FeatureKind::Form { definition, .. } => Some(definition.as_ref()),
                _ => None,
            })
            .unwrap();
        assert_eq!(saved.cage, accepted.cage, "the preview and Finish must publish the same sculpt");
        assert_eq!(saved.profile, accepted.profile);
    }
    #[test]
    fn profile_preview_uses_filled_cad_and_projects_guide_edges_on_that_surface() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top})).unwrap();
        let p = app.session.forms_prototype.as_ref().unwrap().surface.vertices[4] + Vec3::Z;
        app.session.execute("form.prototype.set_surface_vertices", &json!({"vertices":[4],"positions":[[p.x,p.y,p.z]]})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let definition = form.definition();
        let source = Source { document: app.session.doc.clone(), before: app.session.model.state(), moves: Default::default() };
        let Computed { mesh, report, .. } = compute_context(definition, 0.01, Some(source)).unwrap();
        assert_eq!(mesh.normals.len(), mesh.positions.len());
        assert_eq!(mesh.edges.len(), form.surface.boundaries.len());
        let point_key = |point: Vec3| [point.x, point.y, point.z].map(|coordinate| if coordinate == 0.0 { 0 } else { coordinate.to_bits() });
        let edge_key = |a, b| {
            let mut key = [point_key(a), point_key(b)];
            key.sort();
            key
        };
        let triangle_edges: BTreeSet<_> = mesh
            .triangles
            .iter()
            .flat_map(|triangle| {
                let [a, b, c] = mesh.tri(triangle).unwrap();
                [edge_key(a, b), edge_key(b, c), edge_key(c, a)]
            })
            .collect();
        for (edge, curve) in form.cage.edges().unwrap().iter().zip(&mesh.edges) {
            assert!(curve.len() >= 2);
            assert!(curve.first().unwrap().dist(form.surface.vertices[edge[0]]) < 0.01);
            assert!(curve.last().unwrap().dist(form.surface.vertices[edge[1]]) < 0.01);
            for segment in curve.windows(2) {
                assert!(triangle_edges.contains(&edge_key(segment[0], segment[1])), "guide and rendered skin must use the same exact nodes");
            }
        }
        assert!(report.sampled_deviation_mm <= 0.01);
        assert_eq!(report.patches, 4);
        assert!(mesh.edge_faces.iter().all(|faces| !faces.is_empty() && faces.iter().all(|&id| id < 4)));
        assert!(mesh.edge_faces.iter().any(|faces| faces.len() == 2));
        assert!(mesh.edges.iter().any(|e| e[e.len() / 2].dist_to_segment(e[0], *e.last().unwrap()) > 1e-3));
        let tolerance = controls::tolerance(&app).unwrap();
        let current = key(&app, form.revision, tolerance);
        app.forms_prototype.live.index = Some(Arc::new(solidcraft_engine::geom::MeshIndex::new(&mesh)));
        app.forms_prototype.live.ready = Some((current, mesh, report));
        fit(&mut app);
        let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let point = app.session.forms_prototype.as_ref().unwrap().surface.vertices[4];
        let screen = proj.to_screen(point).unwrap();
        assert!(matches!(surface::nearest_with(&app, &proj, screen, Pick::Face), Some(Element::Face(0..=3))));
        assert_eq!(surface::nearest_with(&app, &proj, screen, Pick::Vertex), Some(Element::Vertex(4)));
        app.forms_prototype.elements = [Element::Surface].into_iter().collect();
        selection::sync(&mut app);
        assert_eq!(app.forms_prototype.selected.len(), 9);
    }
    #[test]
    fn live_cad_drag_preference_keeps_the_fast_path_and_can_request_a_fit_while_held() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        assert!(app.preferences.forms_mesh_while_dragging);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: Pos2::new(30.0, 30.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |_| {
            assert!(ctx.input(|i| i.pointer.primary_down()));
            tick(&mut app, &ctx);
        });
        assert!(app.forms_prototype.live.pending.is_none());
        app.preferences.forms_mesh_while_dragging = false;
        let _ = ctx.run_ui(Default::default(), |_| tick(&mut app, &ctx));
        assert!(app.forms_prototype.live.pending.is_some());
        let f = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(app.forms_prototype.live.requested, Some((f.revision, 0.01_f64.to_bits())));
        assert_eq!(f.inspect()["undo"], 0);
    }
    #[test]
    fn a_rigid_move_reuses_fitted_cad_without_starting_a_worker() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        install_for_test(&mut app);
        let original = mesh(&app).unwrap().positions.clone();
        let exact = selection_geometry(&app).unwrap();
        let owners = exact.faces.clone();
        let native_point = from_kernel(solidcraft_engine::doc::kernel::face_geometry(&exact.body, 0).unwrap().surface.eval(0.37, 0.43).0);
        let source = exact.definition.as_ref().clone();
        let logical_points = exact.points.as_ref().unwrap().clone();
        let document = app.session.doc.clone();
        let matrix = solidcraft_engine::doc::rigid(Vec3::new(30.0, 20.0, 10.0), Vec3::ZERO, Vec3::X, 0.4);
        app.session.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
        let ctx = egui::Context::default();
        tick(&mut app, &ctx);
        assert!(app.forms_prototype.live.pending.is_none());
        assert_eq!(app.forms_prototype.live.placement_reuses, 1);
        let exact = selection_geometry(&app).unwrap();
        assert_eq!(exact.faces, owners);
        let placed = from_kernel(solidcraft_engine::doc::kernel::face_geometry(&exact.body, 0).unwrap().surface.eval(0.37, 0.43).0);
        assert!(placed.dist(solidcraft_engine::doc::apply_point(&matrix, native_point)) < 1e-9);
        assert_eq!(*exact.definition, app.session.forms_prototype.as_ref().unwrap().definition());
        assert_eq!(exact.definition.cage, source.cage);
        for (&id, &point) in &logical_points {
            assert!(exact.points.as_ref().unwrap()[&id].dist(solidcraft_engine::doc::apply_point(&matrix, point)) < 1e-7);
        }
        assert!(Arc::ptr_eq(&app.session.doc, &document));
        let displayed = mesh(&app).unwrap();
        for (p, q) in original.iter().zip(&displayed.positions) {
            assert!(solidcraft_engine::doc::apply_point(&matrix, *p).dist(*q) < 1e-9);
        }
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        tick(&mut app, &ctx);
        assert!(app.forms_prototype.live.pending.is_none());
        let displayed = mesh(&app).unwrap();
        for (p, q) in original.iter().zip(&displayed.positions) {
            assert!(p.dist(*q) < 1e-9);
        }
        let exact = selection_geometry(&app).unwrap();
        let restored = from_kernel(solidcraft_engine::doc::kernel::face_geometry(&exact.body, 0).unwrap().surface.eval(0.37, 0.43).0);
        assert!(restored.dist(native_point) < 1e-9);
        assert_eq!(exact.faces, owners);
        assert_eq!(*exact.definition, source);
        // A fit finishing after an ordinary rigid move uses the same exact
        // relocation path before publishing either representation.
        let computed = compute_context(source.clone(), 0.01, None);
        app.session.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
        accept(&mut app, 0.01, source, computed);
        let exact = selection_geometry(&app).unwrap();
        let arrived = from_kernel(solidcraft_engine::doc::kernel::face_geometry(&exact.body, 0).unwrap().surface.eval(0.37, 0.43).0);
        assert!(arrived.dist(solidcraft_engine::doc::apply_point(&matrix, native_point)) < 1e-9);
        assert_eq!(exact.faces, owners);
        assert!(Arc::ptr_eq(&app.session.doc, &document));
    }
    #[test]
    fn cad_display_keeps_surface_vertices_and_edges_pickable() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({})).unwrap();
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        fit(&mut app);
        let f = app.session.forms_prototype.as_ref().unwrap();
        let Computed { mesh, index, report, .. } = compute_with_body(f.definition(), 0.01).unwrap();
        let key = (f.revision, 0.01_f64.to_bits());
        app.forms_prototype.live.index = Some(index);
        app.forms_prototype.live.ready = Some((key, mesh, report));
        let descriptors = solidcraft_engine::forms_prototype::insertion_curves(&app.session, 0.01, &[]).unwrap();
        curves::install_for_test(&mut app, Ok(descriptors.clone()));
        let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let f = app.session.forms_prototype.as_ref().unwrap();
        let mut vertices = 0;
        let mut edges = 0;
        for (id, &p) in f.surface.vertices.iter().enumerate() {
            if surface::visible(&app, &proj, p) {
                let pos = proj.to_screen(p).unwrap();
                assert_eq!(surface::nearest_with(&app, &proj, pos, Pick::Vertex), Some(Element::Vertex(id)));
                vertices += 1;
            }
        }
        for curve in &descriptors {
            let p = curve.at(0.5).unwrap();
            if surface::visible(&app, &proj, p) {
                let pos = proj.to_screen(p).unwrap();
                assert_eq!(surface::nearest_with(&app, &proj, pos, Pick::Edge), Some(Element::Edge(curve.edge)));
                edges += 1;
            }
        }
        assert!(vertices >= 4 && edges >= 4);
    }
    #[test]
    fn release_preview_has_a_cad_face_id_for_every_triangle() {
        let parameters =
            solidcraft_engine::geom::subdivision::primitives::Parameters { segments: [1; 3], dimensions: [10.; 3], ..Default::default() };
        let definition = solidcraft_engine::doc::forms::Definition {
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            placement: Default::default(),
            cage: parameters.build().unwrap(),
            parameters,
            symmetry: None,
            hierarchy: None,
        };
        let Computed { mesh, report, .. } = compute_with_body(definition, 0.05).unwrap();
        assert_eq!(report.patches, 6);
        assert!(report.solid);
        assert_eq!(mesh.tri_face.iter().copied().collect::<BTreeSet<_>>(), (0..6).collect());
        assert_eq!(mesh.edges.iter().enumerate().filter(|(i, _)| !mesh.seams.get(*i).copied().unwrap_or(false)).count(), 12);
    }
}
