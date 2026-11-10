//! Context Pointer geometry notes, saved in the existing reverse-DNS metadata extension point.
//! No geometry edits, format change, external server or silent nearest-face reattachment.
use std::collections::{BTreeMap, BTreeSet};

use egui::{Align2, FontId, Stroke};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use solidcore_record::Vec3;
use solidcraft_engine::doc::{self, ModelBody};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::{Sel, Session};
use solidcraft_viewport::Camera;

use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;
use crate::viewport::{Hit, Proj};
use solidcraft_theme::Tokens;
use solidcraft_ui_text::tr;

pub(crate) const KEY: &str = "com.tutorintelligence.review";
const MAX_NOTES: usize = 128;
const MAX_TEXT: usize = 4096;
const MAX_TARGETS: usize = 32;
const VERSION: u32 = 2;
type Hashes = BTreeMap<String, (String, Option<String>)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    #[default]
    Open,
    ReadyForReview,
    Resolved,
}

impl Status {
    const ALL: [Self; 3] = [Self::Open, Self::ReadyForReview, Self::Resolved];
    fn label(self) -> &'static str {
        tr(match self {
            Self::Open => "ui.review.open",
            Self::ReadyForReview => "ui.review.ready",
            Self::Resolved => "ui.review.resolved",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Locator {
    Face {
        index: usize,
        name: String,
        #[serde(default, flatten)]
        extra: Map<String, Value>,
    },
    Edge {
        index: usize,
        name: String,
        topology: String,
        #[serde(default, flatten)]
        extra: Map<String, Value>,
    },
    Vertex {
        edge_index: usize,
        edge_name: String,
        endpoint: Endpoint,
        source_point: Vec3,
        topology: String,
        #[serde(default, flatten)]
        extra: Map<String, Value>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Endpoint {
    Start,
    End,
}

impl Endpoint {
    fn point(self, edge: &[Vec3]) -> Option<Vec3> {
        match self {
            Self::Start => edge.first().copied(),
            Self::End => edge.last().copied(),
        }
    }
}

impl Locator {
    fn label(&self) -> &'static str {
        tr(match self {
            Self::Face { .. } => "ui.review.face",
            Self::Edge { .. } => "ui.review.edge",
            Self::Vertex { .. } => "ui.review.vertex",
        })
    }

    fn valid(&self) -> bool {
        match self {
            Self::Face { name, .. } => name.len() <= 4096,
            Self::Edge { name, topology, .. } => name.len() <= 4096 && valid_hash(topology),
            Self::Vertex { edge_name, source_point, topology, .. } => {
                edge_name.len() <= 4096 && valid_hash(topology) && source_point.is_finite() && source_point.len() <= 1e15
            }
        }
    }
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Anchor {
    pub body: String,
    pub component: u64,
    pub occurrence_path: Vec<u64>,
    pub feature: u64,
    pub locator: Locator,
    /// SHA-256 of the definition's reference mesh, independent of occurrence placement.
    /// Any changed geometry is conservatively stale, even when the face name survived.
    pub geometry: String,
    /// Definition geometry to occurrence-local coordinates. A changed internal transfer is
    /// not an occurrence move and must not leave the pin behind on another surface.
    pub definition_offset: doc::Mat,
    pub local_point: Vec3,
    pub original_point: Vec3,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Note {
    pub id: u64,
    pub text: String,
    pub status: Status,
    /// The clicked target comes first; all targets share this note and status.
    pub anchors: Vec<Anchor>,
    /// Kept as JSON so camera fields added by another compatible writer survive edits.
    pub camera: Value,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Notebook {
    pub version: u32,
    pub next_id: u64,
    pub notes: Vec<Note>,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Notebook {
    fn default() -> Self {
        Self { version: VERSION, next_id: 1, notes: Vec::new(), extra: Map::new() }
    }
}

fn refusal(id: &str) -> String {
    tr(id).to_string()
}

fn saved_camera(note: &Note) -> Option<Camera> {
    serde_json::from_value::<Camera>(note.camera.clone()).ok().filter(|c| {
        c.is_valid()
            && c.target.len() <= 1e15
            && c.distance <= 1e15
            && c.pitch.abs() <= std::f64::consts::FRAC_PI_2
            && c.fov.is_finite()
            && (0.0..std::f64::consts::PI).contains(&c.fov)
    })
}

impl Notebook {
    pub(crate) fn read(s: &Session) -> Result<Self, String> {
        let Some(value) = s.doc.metadata.get(KEY) else { return Ok(Self::default()) };
        if serde_json::to_vec(value).map_err(|_| refusal("ui.review.invalid"))?.len() > doc::metadata::MAX_VALUE_BYTES {
            return Err(refusal("ui.review.too_large"));
        }
        let mut value = value.clone();
        match value.get("version").and_then(Value::as_u64) {
            Some(1) => {
                // The document is untouched until an explicit note edit is saved. Keep the
                // old face fingerprint and every unknown field through the metadata upgrade.
                let notes = value.get_mut("notes").and_then(Value::as_array_mut).ok_or_else(|| refusal("ui.review.invalid"))?;
                for note in notes {
                    let note = note.as_object_mut().ok_or_else(|| refusal("ui.review.invalid"))?;
                    if note.contains_key("anchors") {
                        return Err(refusal("ui.review.invalid"));
                    }
                    let mut anchor = note.remove("anchor").ok_or_else(|| refusal("ui.review.invalid"))?;
                    let object = anchor.as_object_mut().ok_or_else(|| refusal("ui.review.invalid"))?;
                    if object.contains_key("locator") {
                        return Err(refusal("ui.review.invalid"));
                    }
                    let index = object.remove("face_index").ok_or_else(|| refusal("ui.review.invalid"))?;
                    let name = object.remove("face_name").ok_or_else(|| refusal("ui.review.invalid"))?;
                    object.insert("locator".into(), json!({"kind":"face", "index":index, "name":name}));
                    note.insert("anchors".into(), json!([anchor]));
                }
                if let Some(object) = value.as_object_mut() {
                    object.insert("version".into(), json!(VERSION));
                }
            }
            Some(2) => {}
            _ => return Err(refusal("ui.review.version_error")),
        }
        let book: Self = serde_json::from_value(value).map_err(|_| refusal("ui.review.invalid"))?;
        book.validate()?;
        Ok(book)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != VERSION || self.notes.len() > MAX_NOTES || self.next_id == 0 || self.next_id > u64::from(u32::MAX) {
            return Err(refusal("ui.review.invalid"));
        }
        let mut ids = BTreeSet::new();
        for n in &self.notes {
            if n.id == 0
                || n.id >= self.next_id
                || !ids.insert(n.id)
                || n.text.trim().is_empty()
                || n.text.len() > MAX_TEXT
                || n.text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                || n.anchors.is_empty()
                || n.anchors.len() > MAX_TARGETS
                || saved_camera(n).is_none()
            {
                return Err(refusal("ui.review.invalid"));
            }
            for a in &n.anchors {
                if a.body.is_empty()
                    || a.body.len() > 1024
                    || !a.locator.valid()
                    || a.occurrence_path.len() > 64
                    || !valid_hash(&a.geometry)
                    || !a.local_point.is_finite()
                    || !a.original_point.is_finite()
                    || !a.definition_offset.iter().flatten().all(|v| v.is_finite())
                    || a.local_point.len() > 1e15
                    || a.original_point.len() > 1e15
                {
                    return Err(refusal("ui.review.invalid"));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn value(&self) -> Result<Value, String> {
        self.validate()?;
        let value = serde_json::to_value(self).map_err(|_| refusal("ui.review.invalid"))?;
        // Exercise the metadata depth/value/total-size validator before touching the document.
        let mut checked = doc::metadata::Metadata::default();
        checked.set(doc::metadata::Owner::Design, KEY, value.clone()).map_err(|e| e.to_string())?;
        Ok(value)
    }
}

fn fingerprint(body: &ModelBody) -> String {
    let mesh = body.mesh();
    let mut h = Sha256::new();
    for n in [mesh.positions.len(), mesh.triangles.len(), mesh.tri_face.len()] {
        h.update((n as u64).to_le_bytes());
    }
    for p in &mesh.positions {
        for v in [p.x, p.y, p.z] {
            h.update(v.to_bits().to_le_bytes());
        }
    }
    for i in mesh.triangles.iter().flatten().chain(&mesh.tri_face) {
        h.update(i.to_le_bytes());
    }
    format!("{:x}", h.finalize())
}

/// Separate from the version-1 surface hash: edge order, polylines and adjacency
/// must also stay unchanged before an edge or endpoint can resolve.
fn edge_fingerprint(body: &ModelBody) -> String {
    let mesh = body.mesh();
    let mut h = Sha256::new();
    h.update((mesh.edges.len() as u64).to_le_bytes());
    for edge in &mesh.edges {
        h.update((edge.len() as u64).to_le_bytes());
        for p in edge {
            for v in [p.x, p.y, p.z] {
                h.update(v.to_bits().to_le_bytes());
            }
        }
    }
    h.update((mesh.edge_faces.len() as u64).to_le_bytes());
    for faces in &mesh.edge_faces {
        h.update((faces.len() as u64).to_le_bytes());
        for index in faces {
            h.update(index.to_le_bytes());
        }
    }
    format!("{:x}", h.finalize())
}

/// Only floating-point transform roundoff is tolerated. This is coordinate
/// equivalence, never a nearest-vertex or nearest-surface search.
fn same_point(a: Vec3, b: Vec3) -> bool {
    a.is_finite() && b.is_finite() && a.dist(b) <= 64.0 * f64::EPSILON * (1.0 + a.len().max(b.len()))
}

/// Review picks retain the hit point on an edge; ordinary modelling picks keep
/// their established midpoint behavior in viewport::hit_sel.
pub(crate) fn picked(hit: &Hit) -> Option<Sel> {
    match hit {
        Hit::Face { body, index, point } => Some(Sel::Face { body: body.clone(), index: *index, point: *point }),
        Hit::Edge { body, index, point, .. } => Some(Sel::Edge { body: body.clone(), index: *index, point: *point }),
        Hit::Vertex { body, point } if !body.is_empty() => Some(Sel::Vertex { body: body.clone(), point: *point }),
        _ => None,
    }
}

pub(crate) fn capture(s: &Session, selection: &Sel) -> Result<Anchor, String> {
    let (body, point) = match selection {
        Sel::Face { body, point, .. } | Sel::Edge { body, point, .. } | Sel::Vertex { body, point } if !body.is_empty() => (body, *point),
        _ => return Err(refusal("ui.review.pick_face")),
    };
    let model = s.model.state();
    let placed = s.placed_document();
    let (_, definition, component, path, _) =
        placed.body_frames_with_source(&model).into_iter().find(|f| f.0 == *body).ok_or_else(|| refusal("ui.review.stale"))?;
    let source = model.body(&definition).ok_or_else(|| refusal("ui.review.stale"))?;
    let matrix = placed.path_transform(component, &path).ok_or_else(|| refusal("ui.review.stale"))?;
    let inverse = doc::mat_inverse(&matrix).ok_or_else(|| refusal("ui.review.stale"))?;
    let definition_offset = placed.model_body_offset(source).copied().unwrap_or(doc::IDENTITY);
    let mut local_point = doc::apply_point(&inverse, point);
    if !local_point.is_finite() || !point.is_finite() {
        return Err(refusal("ui.review.invalid"));
    }
    let locator = match selection {
        Sel::Face { index, .. } => Locator::Face {
            index: *index,
            name: doc::naming::face_names(source).get(*index).cloned().ok_or_else(|| refusal("ui.review.stale"))?,
            extra: Map::new(),
        },
        Sel::Edge { index, .. } => Locator::Edge {
            index: *index,
            name: doc::naming::edge_names(source).get(*index).cloned().ok_or_else(|| refusal("ui.review.stale"))?,
            topology: edge_fingerprint(source),
            extra: Map::new(),
        },
        Sel::Vertex { .. } => {
            let mesh = source.mesh();
            let world = doc::mat_mul(&matrix, &definition_offset);
            let (edge_index, endpoint, source_point) = mesh
                .edges
                .iter()
                .enumerate()
                .find_map(|(index, edge)| {
                    [Endpoint::Start, Endpoint::End].into_iter().find_map(|endpoint| {
                        let source_point = endpoint.point(edge)?;
                        same_point(doc::apply_point(&world, source_point), point).then_some((index, endpoint, source_point))
                    })
                })
                .ok_or_else(|| refusal("ui.review.stale"))?;
            local_point = doc::apply_point(&definition_offset, source_point);
            Locator::Vertex {
                edge_index,
                edge_name: doc::naming::edge_names(source).get(edge_index).cloned().ok_or_else(|| refusal("ui.review.stale"))?,
                endpoint,
                source_point,
                topology: edge_fingerprint(source),
                extra: Map::new(),
            }
        }
        _ => return Err(refusal("ui.review.pick_face")),
    };
    Ok(Anchor {
        body: definition,
        component,
        occurrence_path: path,
        feature: source.feature,
        locator,
        geometry: fingerprint(source),
        definition_offset,
        local_point,
        original_point: point,
        extra: Map::new(),
    })
}

/// Resolve only the recorded definition and exact occurrence chain, never a nearest body/face.
#[cfg(test)]
pub(crate) fn resolve(s: &Session, anchor: &Anchor) -> Option<Vec3> {
    resolve_cached(s, anchor, &mut BTreeMap::new())
}

fn resolve_cached(s: &Session, a: &Anchor, hashes: &mut Hashes) -> Option<Vec3> {
    let model = s.model.state();
    let source = model.body(&a.body)?;
    let placed = s.placed_document();
    if placed.model_body_component(source) != a.component
        || source.feature != a.feature
        || placed.model_body_offset(source).copied().unwrap_or(doc::IDENTITY) != a.definition_offset
    {
        return None;
    }
    let (surface, edges) = hashes.entry(a.body.clone()).or_insert_with(|| (fingerprint(source), None));
    if surface != &a.geometry {
        return None;
    }
    match &a.locator {
        Locator::Face { index, name, .. } => {
            if doc::naming::face_names(source).get(*index)? != name {
                return None;
            }
        }
        Locator::Edge { index, name, topology, .. } => {
            if doc::naming::edge_names(source).get(*index)? != name || edges.get_or_insert_with(|| edge_fingerprint(source)) != topology {
                return None;
            }
        }
        Locator::Vertex { edge_index, edge_name, endpoint, source_point, topology, .. } => {
            if doc::naming::edge_names(source).get(*edge_index)? != edge_name
                || edges.get_or_insert_with(|| edge_fingerprint(source)) != topology
                || endpoint.point(source.mesh().edges.get(*edge_index)?)? != *source_point
                || !same_point(doc::apply_point(&a.definition_offset, *source_point), a.local_point)
            {
                return None;
            }
        }
    }
    let matrix = placed.path_transform(a.component, &a.occurrence_path)?;
    let point = doc::apply_point(&matrix, a.local_point);
    point.is_finite().then_some(point)
}

pub(crate) struct Draft {
    pub note: Note,
    /// Optimistic conflict check: an edit may not overwrite another writer's change.
    pub base: Option<Note>,
}

pub struct ReviewNotes {
    uid: u64,
    pub(crate) draft: Option<Draft>,
    focus_text: bool,
    error: Option<String>,
    cache: Option<Cache>,
}

struct Cache {
    revision: u64,
    // Hold the world, not just its address: an allocator can reuse a dropped state's address.
    world: std::sync::Arc<doc::ModelState>,
    book: Result<Notebook, String>,
    points: BTreeMap<u64, Vec<Option<Vec3>>>,
    model: std::sync::Arc<doc::ModelState>,
    hashes: Hashes,
}

pub(crate) fn open(app: &mut SolveApp) {
    if app.review_notes.is_none() {
        app.review_notes = Some(ReviewNotes { uid: app.session.uid, draft: None, focus_text: false, error: None, cache: None });
    }
}

/// A context-menu action starts an editor at its own exact hit, independent of the
/// selection retained for the other modelling commands in that menu.
pub(crate) fn add_at(app: &mut SolveApp, selections: &[Sel]) {
    if !can_add(app) {
        return;
    }
    let draft = new_draft_at(app, selections);
    open(app);
    if let Some(state) = app.review_notes.as_mut() {
        match draft {
            Ok(draft) => {
                state.draft = Some(draft);
                state.focus_text = true;
                state.error = None;
            }
            Err(error) => state.error = Some(error),
        }
    }
}

pub(crate) fn can_add(app: &SolveApp) -> bool {
    app.dialog.is_none() && app.tool.is_none() && app.session.active_sketch.is_none() && app.review_notes.as_ref().is_none_or(|r| r.draft.is_none())
}

pub(crate) fn picking(app: &SolveApp) -> bool {
    app.review_notes.as_ref().is_some_and(|r| r.uid == app.session.uid)
        && app.dialog.is_none()
        && app.tool.is_none()
        && app.session.active_sketch.is_none()
}

impl ReviewNotes {
    fn refresh(&mut self, s: &Session) {
        let world = match s.world_state() {
            Ok(world) => world,
            Err(error) => {
                self.cache = None;
                self.error = Some(error.to_string());
                return;
            }
        };
        if self.cache.as_ref().is_some_and(|c| c.revision == s.revision && std::sync::Arc::ptr_eq(&c.world, &world)) {
            return;
        }
        let book = Notebook::read(s);
        let model = s.model.state();
        let mut hashes = self.cache.as_ref().filter(|c| std::sync::Arc::ptr_eq(&c.model, &model)).map(|c| c.hashes.clone()).unwrap_or_default();
        let points = book
            .as_ref()
            .map(|b| b.notes.iter().map(|n| (n.id, n.anchors.iter().map(|a| resolve_cached(s, a, &mut hashes)).collect())).collect())
            .unwrap_or_default();
        self.cache = Some(Cache { revision: s.revision, world, book, points, model, hashes });
    }
}

pub(crate) fn new_draft(app: &SolveApp) -> Result<Draft, String> {
    let primary = app.session.selection.iter().rev().find(|s| geometry_selection(s)).ok_or_else(|| refusal("ui.review.pick_face"))?;
    new_draft_at(app, &selected_targets(&app.session.selection, primary))
}

fn geometry_selection(selection: &Sel) -> bool {
    matches!(selection, Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } if !body.is_empty())
}

/// A right-click already in a geometric multi-selection retains every selected
/// entity, with the clicked entity's exact hit point replacing its older point.
pub(crate) fn selected_targets(selection: &[Sel], primary: &Sel) -> Vec<Sel> {
    let mut targets = vec![primary.clone()];
    if selection.iter().any(|s| crate::selection::same_item(s, primary)) {
        for selected in selection.iter().filter(|s| geometry_selection(s)) {
            if !targets.iter().any(|s| crate::selection::same_item(s, selected)) {
                targets.push(selected.clone());
            }
        }
    }
    targets
}

fn new_draft_at(app: &SolveApp, selections: &[Sel]) -> Result<Draft, String> {
    let book = Notebook::read(&app.session)?;
    if book.notes.len() >= MAX_NOTES {
        return Err(refusal("ui.review.limit"));
    }
    if selections.is_empty() || selections.len() > MAX_TARGETS {
        return Err(refusal("ui.review.target_limit"));
    }
    let anchors = selections.iter().map(|selection| capture(&app.session, selection)).collect::<Result<Vec<_>, _>>()?;
    let camera = serde_json::to_value(app.cam).map_err(|_| refusal("ui.review.invalid"))?;
    Ok(Draft { note: Note { id: 0, text: String::new(), status: Status::Open, anchors, camera, extra: Map::new() }, base: None })
}

/// One existing metadata command, hence one normal undo step and no model evaluation.
pub(crate) fn save(app: &mut SolveApp, draft: &Draft, delete: bool) -> Result<(), String> {
    let mut book = Notebook::read(&app.session)?;
    if let Some(base) = &draft.base {
        let position = book.notes.iter().position(|n| n.id == base.id).ok_or_else(|| refusal("ui.review.conflict"))?;
        if book.notes.get(position) != Some(base) {
            return Err(refusal("ui.review.conflict"));
        }
        if delete {
            book.notes.remove(position);
        } else if let Some(note) = book.notes.get_mut(position) {
            *note = draft.note.clone();
        }
    } else if !delete {
        if book.notes.len() >= MAX_NOTES {
            return Err(refusal("ui.review.limit"));
        }
        let mut note = draft.note.clone();
        note.id = book.next_id;
        book.next_id = book.next_id.checked_add(1).ok_or_else(|| refusal("ui.review.limit"))?;
        book.notes.push(note);
    }
    let value = book.value()?;
    app.run("document.metadata.set", json!({"key": KEY, "value": value}))?;
    Ok(())
}

fn button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let response = ui_kit::text_button(ui, tr(label), enabled);
    crate::scenario::publish_target(id, response.rect);
    response.clicked()
}

fn primary_button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> bool {
    let response = ui.add_enabled_ui(enabled, |ui| ui_kit::button(ui, tr(label), ui_kit::Button::Primary)).inner;
    crate::scenario::publish_target(id, response.rect);
    response.clicked()
}

fn recall(app: &mut SolveApp, note: &Note) {
    if let Some(mut camera) = saved_camera(note) {
        // Lens and up-axis follow the same user preferences as all viewport navigation.
        camera.fov = app.cam.fov;
        camera.up = app.cam.up;
        app.animate_to(camera);
    }
}

fn target_summary(note: &Note) -> String {
    let faces = note.anchors.iter().filter(|a| matches!(a.locator, Locator::Face { .. })).count();
    let edges = note.anchors.iter().filter(|a| matches!(a.locator, Locator::Edge { .. })).count();
    let vertices = note.anchors.len() - faces - edges;
    solidcraft_ui_text::format(
        "ui.review.targets",
        &[
            ("count", &note.anchors.len().to_string()),
            ("faces", &faces.to_string()),
            ("edges", &edges.to_string()),
            ("vertices", &vertices.to_string()),
        ],
    )
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut state) = app.review_notes.take() else { return };
    if state.uid != app.session.uid {
        return;
    }
    state.refresh(&app.session);
    let book = state.cache.as_ref().map(|c| c.book.clone()).unwrap_or_else(|| Notebook::read(&app.session));
    let closed = ui_kit::Window::new(egui::Id::unique("sc_review_notes"), "list", tr("cmd.document.review"))
        .cards(1.5)
        .keep_open_on_escape()
        .placed(Align2::RIGHT_TOP, egui::pos2(ctx.content_rect().right() - Tokens::get().space_16, ctx.content_rect().top() + 150.0))
        .show_body(ctx, |ui| {
            ui_kit::text(ui, tr("ui.review.hint"), ui_kit::Text::Note("review_hint"));
            let book = match &book {
                Ok(book) => book,
                Err(error) => {
                    ui_kit::text(ui, error, ui_kit::Text::Report("review_data_error", true));
                    return;
                }
            };
            let target_selected = app.session.selection.iter().any(geometry_selection);
            ui.horizontal_wrapped(|ui| {
                if primary_button(ui, "review:add", "ui.review.add", state.draft.is_none() && target_selected) {
                    match new_draft(app) {
                        Ok(draft) => {
                            state.draft = Some(draft);
                            state.focus_text = true;
                            state.error = None;
                        }
                        Err(error) => state.error = Some(error),
                    }
                }
                if button(ui, "review:undo", "ui.review.undo", !app.session.undo.is_empty()) {
                    match app.run("edit.undo", json!({})) {
                        Ok(_) => {
                            state.draft = None;
                            state.error = None;
                        }
                        Err(error) => state.error = Some(error),
                    }
                }
            });
            if !target_selected && state.draft.is_none() {
                ui_kit::text(ui, tr("ui.review.pick_face"), ui_kit::Text::Note("review_pick_face"));
            }
            if book.notes.is_empty() {
                ui_kit::text(ui, tr("ui.review.empty"), ui_kit::Text::Note("review_empty"));
            }
            egui::ScrollArea::vertical().id_salt("review_list").max_height(ui_kit::row_height() * 7.0).show(ui, |ui| {
                for note in &book.notes {
                    let stale = state
                        .cache
                        .as_ref()
                        .and_then(|c| c.points.get(&note.id))
                        .map(|points| points.iter().filter(|p| p.is_none()).count())
                        .unwrap_or(note.anchors.len());
                    let title = format!("#{} · {}{}", note.id, note.status.label(), if stale > 0 { " ⚠" } else { "" });
                    let r = ui_kit::text_button(ui, &title, state.draft.is_none());
                    crate::scenario::publish_target(&format!("review:note:{}", note.id), r.rect);
                    if r.clicked() {
                        recall(app, note);
                        state.draft = Some(Draft { note: note.clone(), base: Some(note.clone()) });
                        state.focus_text = true;
                        state.error = None;
                    }
                    ui_kit::text(ui, &note.text, ui_kit::Text::Cell);
                    ui_kit::text(ui, &target_summary(note), ui_kit::Text::Note("review_targets"));
                    if stale > 0 {
                        let message = solidcraft_ui_text::format(
                            "ui.review.partial_stale",
                            &[("stale", &stale.to_string()), ("total", &note.anchors.len().to_string())],
                        );
                        ui_kit::text(ui, &message, ui_kit::Text::Report("review_stale", true));
                    }
                    ui.separator();
                }
            });
            if let Some(draft) = &mut state.draft {
                ui.separator();
                ui_kit::text(ui, &target_summary(&draft.note), ui_kit::Text::Cell);
                if let Some(primary) = draft.note.anchors.first() {
                    let label = solidcraft_ui_text::format("ui.review.primary_target", &[("type", primary.locator.label()), ("body", &primary.body)]);
                    ui_kit::text(ui, &label, ui_kit::Text::Cell);
                }
                let text = ui_kit::multiline_text_field(ui, tr("ui.review.text"), egui::Id::unique("review_text"), &mut draft.note.text);
                crate::scenario::publish_target("review:text", text.rect);
                if std::mem::take(&mut state.focus_text) {
                    text.request_focus();
                }
                let mut status = Status::ALL.iter().position(|s| *s == draft.note.status).unwrap_or_default();
                ui_kit::row(ui, tr("ui.review.status"), |ui| {
                    ui_kit::segmented(ui, &mut status, &Status::ALL.map(Status::label), Default::default());
                });
                if let Some(value) = Status::ALL.get(status) {
                    draft.note.status = *value;
                }
                let mut finished = false;
                ui.horizontal_wrapped(|ui| {
                    if primary_button(ui, "review:save", "ui.review.save", !draft.note.text.trim().is_empty() && draft.note.text.len() <= MAX_TEXT) {
                        match save(app, draft, false) {
                            Ok(()) => {
                                finished = true;
                                state.error = None;
                            }
                            Err(error) => state.error = Some(error),
                        }
                    }
                    if button(ui, "review:view", "ui.review.view", true) {
                        recall(app, &draft.note);
                    }
                    if button(ui, "review:delete", "ui.review.delete", draft.base.is_some()) {
                        match save(app, draft, true) {
                            Ok(()) => {
                                finished = true;
                                state.error = None;
                            }
                            Err(error) => state.error = Some(error),
                        }
                    }
                    if button(ui, "review:cancel", "ui.review.cancel", true) {
                        finished = true;
                        state.error = None;
                    }
                });
                if draft.note.text.len() > MAX_TEXT {
                    ui_kit::text(ui, tr("ui.review.text_limit"), ui_kit::Text::Report("review_text_limit", true));
                }
                if finished {
                    state.draft = None;
                }
            }
            if let Some(error) = &state.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("review_error", true));
            }
            ui_kit::text(ui, tr("ui.review.saved_with_design"), ui_kit::Text::Note("review_save_hint"));
        });
    if !closed {
        app.review_notes = Some(state);
    }
}

pub(crate) fn overlay(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let Some(mut state) = app.review_notes.take() else { return };
    if state.uid != app.session.uid {
        return;
    }
    state.refresh(&app.session);
    let t = Tokens::get();
    if let Some(Cache { book: Ok(book), points, .. }) = &state.cache {
        for note in &book.notes {
            for (index, point) in points.get(&note.id).into_iter().flatten().enumerate() {
                let Some(point) = *point else { continue };
                let Some(at) = proj.to_screen(point).filter(|p| proj.rect.contains(*p)) else { continue };
                let centre = at - egui::vec2(0.0, t.space_16);
                let color = match note.status {
                    Status::Open => t.accent,
                    Status::ReadyForReview => t.warning,
                    Status::Resolved => t.form_success,
                };
                painter.line_segment([at, centre], Stroke::new(2.0, color));
                painter.circle_filled(at, 3.0, color);
                painter.circle_filled(centre, t.space_12, color);
                painter.circle_stroke(centre, t.space_12, Stroke::new(1.0, t.text));
                painter.text(centre, Align2::CENTER_CENTER, note.id.to_string(), FontId::proportional(t.font_small), t.on_accent);
                let rect = egui::Rect::from_center_size(centre, egui::vec2(t.space_12 * 2.0, t.space_12 * 2.0));
                let response =
                    ui.interact(rect, egui::Id::unique(("review_pin", note.id, index)), egui::Sense::click()).backing_on_hover_text(&note.text);
                crate::scenario::publish_target(&format!("review:pin:{}:{}", note.id, index), rect);
                if index == 0 {
                    crate::scenario::publish_target(&format!("review:pin:{}", note.id), rect);
                }
                if response.clicked() && state.draft.is_none() {
                    recall(app, note);
                    state.draft = Some(Draft { note: note.clone(), base: Some(note.clone()) });
                    state.focus_text = true;
                    state.error = None;
                }
            }
        }
    }
    app.review_notes = Some(state);
}
