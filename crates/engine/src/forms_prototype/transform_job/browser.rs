//! Bounded materialized Form target results for the browser compute worker.
//! The page retains the command stamp and history; only the worker evaluates geometry.
use super::*;
use crate::geom::{
    Mesh, MeshBufferLimits, MeshIndex, MeshIndexWire,
    subdivision::{Cage, SurfacePreview, hierarchy::MAX_DEPTH},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solidcore_record::Vec3;
use std::io::Write;

const VERSION: u32 = 1;
const MAX_POINTS: usize = 65_536;
const MAX_FACES: usize = 32_768;
const MAX_CORNERS: usize = 262_144;
const MAX_DISPLAY_POINTS: usize = 100_000;
const MAX_DISPLAY_TRIANGLES: usize = 200_000;
const MAX_DISPLAY_STATIONS: usize = 200_000;

/// Private, versioned worker transfer for the existing Form target commands.
/// This is not a saved-design or recovery format.
pub struct BrowserTransform;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Space {
    Control,
    Surface,
}
impl From<FormTargetSpace> for Space {
    fn from(space: FormTargetSpace) -> Self {
        match space {
            FormTargetSpace::Control => Self::Control,
            FormTargetSpace::Surface => Self::Surface,
        }
    }
}
impl From<Space> for FormTargetSpace {
    fn from(space: Space) -> Self {
        match space {
            Space::Control => Self::Control,
            Space::Surface => Self::Surface,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    version: u32,
    definition: doc::forms::Definition,
    levels: usize,
    space: Space,
    params: Value,
    param_check: String,
    tolerance: Option<f64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Output {
    version: u32,
    input: [u8; 32],
    outcome: std::result::Result<Answer, String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    materialized: Option<Materialized>,
    /// Only shared parameter-check metadata; the page produces its own inspector
    /// so local history, staged objects and editor identity remain authoritative.
    metadata: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Materialized {
    definition: doc::forms::Definition,
    logical: Cage,
    local_vertices: Vec<Vec3>,
    surface: Display,
    levels: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Display {
    mesh: Mesh,
    vertices: Vec<Vec3>,
    boundaries: Vec<([usize; 2], Vec<Vec3>)>,
    index: MeshIndexWire,
    // Mesh intentionally omits these presentation fields from its own serde.
    display_only_edges: bool,
    smooth_edges: Vec<bool>,
    sharp_edges: Vec<bool>,
}

struct Buffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("Form transform exceeds the browser transfer limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    let mut buffer = Buffer { bytes: Vec::new(), limit };
    serde_json::to_writer(&mut buffer, value).map_err(error)?;
    Ok(buffer.bytes)
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn finite(points: &[Vec3]) -> bool {
    points.iter().all(|p| p.is_finite() && [p.x, p.y, p.z].into_iter().all(|v| v.abs() <= 1_000_000.0))
}
fn bounded_cage(cage: &Cage) -> bool {
    cage.points.len() <= MAX_POINTS
        && cage.faces.len() <= MAX_FACES
        && cage.faces.iter().fold(0usize, |n, face| n.saturating_add(face.len())) <= MAX_CORNERS
        && cage.edge_creases.len() <= MAX_CORNERS
        && cage.vertex_creases.len() <= MAX_POINTS
        && finite(&cage.points)
        && cage.faces.iter().flatten().all(|&id| id < cage.points.len())
        && cage.edge_creases.iter().all(|edge| edge.edge.iter().all(|&id| id < cage.points.len()) && edge.sharpness.is_finite())
        && cage.vertex_creases.iter().all(|point| point.vertex < cage.points.len() && point.sharpness.is_finite())
}
fn bounded_definition(definition: &doc::forms::Definition, levels: usize) -> bool {
    levels <= crate::geom::subdivision::MAX_LEVELS
        && bounded_cage(&definition.cage)
        && definition.parameters.path.len() <= MAX_POINTS
        && definition.hierarchy.as_ref().is_none_or(|h| {
            h.depth <= MAX_DEPTH
                && bounded_cage(&h.base)
                && h.leaves.len() <= MAX_FACES
                && h.controls.len() <= MAX_POINTS
                && h.details.len() <= MAX_DEPTH + 1
                && h.details.iter().fold(0usize, |n, details| n.saturating_add(details.len())) <= MAX_POINTS
                && h.details.iter().flatten().all(|detail| detail.vertex < definition.cage.points.len() && detail.delta.is_finite())
                && h.replay_face_bound().is_some_and(|faces| faces <= MAX_FACES)
        })
}
fn bounded_display(display: &Display, logical: &Cage, unowned_face: Option<u32>) -> bool {
    display.mesh.buffers_within(MeshBufferLimits {
        positions: MAX_DISPLAY_POINTS,
        triangles: MAX_DISPLAY_TRIANGLES,
        edges: MAX_CORNERS,
        stations: MAX_DISPLAY_STATIONS,
        faces: logical.faces.len(),
        unowned_face,
    }) && display.vertices.len() <= MAX_POINTS
        && finite(&display.vertices)
        && display.boundaries.len() <= MAX_CORNERS
        && display.boundaries.iter().fold(0usize, |n, (_, curve)| n.saturating_add(curve.len())) <= MAX_DISPLAY_STATIONS
        && display.boundaries.iter().all(|(edge, curve)| edge.iter().all(|&id| id < logical.points.len()) && finite(curve))
        && display.smooth_edges.len() <= display.mesh.edges.len()
        && display.sharp_edges.len() <= display.mesh.edges.len()
}

/// Remove only the coordinate fields the existing target commands may change.
/// Everything else, including private cap ownership/retired slots, is compared
/// with the locally captured source before accepting a materialized result.
fn structure(definition: &doc::forms::Definition) -> Result<Value> {
    let mut value = serde_json::to_value(definition).map_err(error)?;
    let object = value.as_object_mut().ok_or_else(|| error("invalid Form transform source"))?;
    object.remove("placement");
    if let Some(cage) = object.get_mut("cage").and_then(Value::as_object_mut) {
        cage.remove("points");
    }
    if let Some(symmetry) = object.get_mut("symmetry").and_then(Value::as_object_mut) {
        symmetry.remove("origin");
    }
    if let Some(pipe) = object.get_mut("pipe").and_then(Value::as_object_mut) {
        pipe.remove("follow_path");
    }
    if let Some(hierarchy) = object.get_mut("hierarchy").and_then(Value::as_object_mut) {
        hierarchy.remove("details");
        if let Some(base) = hierarchy.get_mut("base").and_then(Value::as_object_mut) {
            base.remove("points");
        }
        if let Some(wall) = hierarchy.get_mut("trim_wall").and_then(Value::as_object_mut) {
            wall.remove("delta");
        }
    }
    Ok(value)
}

impl BrowserTransform {
    /// Maximum encoded source/target request, checked before parsing.
    pub const MAX_REQUEST: usize = 1024 * 1024;
    /// Maximum encoded materialized display and source result.
    pub const MAX_REPLY: usize = 8 * 1024 * 1024;

    /// Worker-only entry. Source reconstruction, inverse solving, preview/BVH
    /// construction and derived CAD qualification all happen here.
    pub fn run(bytes: &[u8]) -> Result<Vec<u8>> {
        if bytes.len() > Self::MAX_REQUEST {
            return Err(error("Form transform exceeds the browser request limit"));
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Self::compute(bytes)))
            .map_err(|_| error("Form transform worker stopped"))
            .and_then(|result| result)
            .map_err(|error| error.to_string());
        encode(&Output { version: VERSION, input: digest(bytes), outcome }, Self::MAX_REPLY)
    }

    fn compute(bytes: &[u8]) -> Result<Answer> {
        let input: Input = serde_json::from_slice(bytes).map_err(error)?;
        if input.version != VERSION || !bounded_definition(&input.definition, input.levels) {
            return Err(error("invalid or oversized browser Form transform source"));
        }
        let check = schema::ParamCheck::parse(&input.param_check).ok_or_else(|| error("invalid browser Form parameter mode"))?;
        let space = FormTargetSpace::from(input.space);
        let mut form = Form::from_definition(input.definition, input.levels, None)?;
        if form.needs_fitted_transform() && !input.tolerance.is_some_and(|t| t.is_finite() && (1e-6..=1.0).contains(&t)) {
            return Err(error("derived Form transform requires a bounded CAD tolerance"));
        }
        let before = form.source_revision();
        let metadata = schema::run_detached_checked(check, &input.params, space.command(), || {
            let mut params: schema::forms_prototype::SetVertices = serde_json::from_value(input.params.clone()).map_err(error)?;
            params.checkpoint = false;
            form.require_editable()?;
            form.apply_transform_targets(&params, space)?;
            if form.source_revision() != before
                && let Some(tolerance) = input.tolerance
            {
                qualify_derived_edit(&form, tolerance)?;
            }
            Ok(Value::Object(Default::default()))
        })?;
        let materialized = if form.source_revision() == before {
            None
        } else {
            let surface = form.surface.as_ref();
            let display = Display {
                mesh: surface.mesh.clone(),
                vertices: surface.vertices.clone(),
                boundaries: surface.boundaries.clone(),
                index: surface.index.to_wire(),
                display_only_edges: surface.mesh.display_only_edges,
                smooth_edges: surface.mesh.smooth_edges.clone(),
                sharp_edges: surface.mesh.sharp_edges.clone(),
            };
            if !bounded_display(&display, &form.logical, form.profile.as_ref().filter(|profile| !profile.partition).map(|_| u32::MAX)) {
                return Err(error("Form transform exceeds the browser display limit"));
            }
            Some(Materialized {
                definition: form.definition(),
                logical: form.logical,
                local_vertices: form.local_vertices.to_vec(),
                surface: display,
                levels: form.levels,
            })
        };
        Ok(Answer { materialized, metadata })
    }
}

impl FormTransformJob {
    /// Capture primary source only. History, staged objects, host hooks and
    /// authoring/document identity remain in this locally retained job.
    pub fn browser_request(&self) -> Result<Vec<u8>> {
        if self.stop.is_stopped() {
            return Err(error("Form transform stopped"));
        }
        self.form.require_editable()?;
        if self.form.extrusion.is_some() {
            return Err(error("this browser transform cannot transfer its retained extrusion gesture yet"));
        }
        let definition = self.form.definition();
        if !bounded_definition(&definition, self.form.levels) {
            return Err(error("Form transform source exceeds the browser work limit"));
        }
        encode(
            &Input {
                version: VERSION,
                definition,
                levels: self.form.levels,
                space: self.space.into(),
                params: self.params.clone(),
                param_check: self.source.param_check.name().into(),
                tolerance: self.derived_edit_tolerance,
            },
            BrowserTransform::MAX_REQUEST,
        )
    }

    /// Hydrate a result without evaluating or rebuilding any geometry on the
    /// page. The existing accept_form_transform subsequently checks the live stamp.
    pub fn with_browser_reply(self, bytes: &[u8]) -> Result<PreparedFormTransform> {
        if bytes.len() > BrowserTransform::MAX_REPLY {
            return Err(error("Form transform exceeds the browser reply limit"));
        }
        let expected = digest(&self.browser_request()?);
        let output: Output = serde_json::from_slice(bytes).map_err(error)?;
        if output.version != VERSION || output.input != expected {
            return Err(error("browser Form transform reply does not match its input"));
        }
        let Answer { materialized, metadata } = output.outcome.map_err(error)?;
        let mut form = self.form;
        let changed = materialized.is_some();
        if let Some(data) = materialized {
            if !bounded_definition(&data.definition, data.levels)
                || !bounded_cage(&data.logical)
                || !finite(&data.local_vertices)
                || data.local_vertices.len() != form.local_vertices.len()
                || data.logical.faces != form.logical.faces
                || data.logical.points.len() != form.logical.points.len()
                || data.logical.edge_creases != form.logical.edge_creases
                || data.logical.vertex_creases != form.logical.vertex_creases
                || data.definition.cage.points.len() != form.cage.points.len()
                || data.levels != form.levels
                || structure(&data.definition)? != structure(&form.definition())?
                || !bounded_display(&data.surface, &data.logical, form.profile.as_ref().filter(|profile| !profile.partition).map(|_| u32::MAX))
                || data.surface.vertices.len() != form.surface.vertices.len()
                || data.surface.boundaries.iter().map(|(edge, _)| edge).ne(form.surface.boundaries.iter().map(|(edge, _)| edge))
            {
                return Err(error("browser Form transform changed its source ownership or display structure"));
            }
            data.definition.placement.validate()?;
            let Display { mut mesh, vertices, boundaries, index, display_only_edges, smooth_edges, sharp_edges } = data.surface;
            mesh.display_only_edges = display_only_edges;
            mesh.smooth_edges = smooth_edges;
            mesh.sharp_edges = sharp_edges;
            let index = MeshIndex::from_wire(index, &mesh).ok_or_else(|| error("invalid browser Form display index"))?;
            let params: schema::forms_prototype::SetVertices = serde_json::from_value(self.params.clone()).map_err(error)?;
            if params.checkpoint {
                form.checkpoint();
            }
            let doc::forms::Definition { imported, pipe, profile, matching, placement, cage, parameters, symmetry, hierarchy } = data.definition;
            form.imported = imported;
            form.pipe = pipe;
            form.profile = profile;
            form.matching = matching;
            form.placement = placement;
            form.cage = cage;
            form.parameters = parameters;
            form.symmetry = symmetry;
            form.hierarchy = hierarchy;
            form.logical = data.logical;
            form.local_vertices = Arc::from(data.local_vertices);
            form.surface = Arc::new(SurfacePreview { mesh, vertices, boundaries, index });
            form.levels = data.levels;
            form.match_report = None;
            form.response = None;
            form.extrusion = None;
            form.redo.clear();
            form.revision = form.revision.wrapping_add(1);
        }
        let mut reply = form.inspect();
        let metadata = metadata
            .as_object()
            .filter(|metadata| metadata.keys().all(|key| key == "param_warnings"))
            .ok_or_else(|| error("invalid browser Form parameter metadata"))?;
        if let Some(warnings) = metadata.get("param_warnings") {
            if !warnings
                .as_array()
                .is_some_and(|warnings| warnings.len() <= 256 && warnings.iter().all(|warning| warning.as_str().is_some_and(|s| s.len() <= 4096)))
            {
                return Err(error("invalid browser Form parameter warnings"));
            }
            reply.as_object_mut().ok_or_else(|| error("invalid Form transform reply"))?.insert("param_warnings".into(), warnings.clone());
        }
        Ok(PreparedFormTransform { source: self.source, form: changed.then_some(form), space: self.space, params: self.params, reply })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::PlaneGeom;
    use serde_json::json;

    fn capped() -> (Session, Vec<usize>) {
        let (session, face) = super::super::tests::capped_form();
        let ids = session.forms_prototype.as_ref().unwrap().logical.faces[face].clone();
        (session, ids)
    }
    fn params(session: &Session, ids: &[usize], mode: usize, checkpoint: bool) -> Value {
        let form = session.forms_prototype.as_ref().unwrap();
        let points = &form.surface.vertices;
        let center = ids.iter().map(|&id| points[id]).fold(Vec3::ZERO, |a, b| a + b) / ids.len() as f64;
        let (move_by, turn, scale) = match mode {
            0 => ([0.0, 0.0, 0.04], [0.0; 3], [1.0; 3]),
            1 => ([0.0; 3], [0.01, 0.0, 0.0], [1.0; 3]),
            2 => ([0.0; 3], [0.0; 3], [1.01, 0.99, 1.0]),
            _ => ([0.0; 3], [0.0; 3], [1.0; 3]),
        };
        let positions: Vec<_> = ids
            .iter()
            .map(|&id| {
                let p =
                    solidcraft_kernel::services::form_transform::numeric(points[id], center, form.placement.axes, move_by, turn, scale, 1.0).unwrap();
                [p.x, p.y, p.z]
            })
            .collect();
        json!({"vertices":ids,"positions":positions,"checkpoint":checkpoint})
    }
    fn roundtrip(session: &mut Session, given: Value) -> Result<(bool, Value)> {
        let job = session.prepare_form_transform(FormTargetSpace::Surface, given)?;
        let request = job.browser_request()?;
        let response = BrowserTransform::run(&request)?;
        let ready = job.with_browser_reply(&response)?;
        session.accept_form_transform(ready)
    }

    #[test]
    fn browser_cap_targets_match_native_source_view_and_local_history() {
        let (source, ids) = capped();
        let original = source.forms_prototype.as_ref().unwrap().definition();
        for mode in 0..3 {
            let mut native = source.scratch();
            let mut browser = source.scratch();
            let given = params(&browser, &ids, mode, true);
            let prepared = native.prepare_form_transform(FormTargetSpace::Surface, given.clone()).unwrap().compute().unwrap();
            native.accept_form_transform(prepared).unwrap();
            let old_undo = browser.forms_prototype.as_ref().unwrap().undo.len();
            let document = browser.doc.clone();
            let before_stamp = browser.forms_prototype.as_ref().unwrap().source_revision();
            let (changed, reply) = roundtrip(&mut browser, given).unwrap();
            assert!(changed);
            assert!(Arc::ptr_eq(&browser.doc, &document));
            let form = browser.forms_prototype.as_ref().unwrap();
            let expected = native.forms_prototype.as_ref().unwrap();
            assert_eq!(form.definition(), expected.definition());
            assert_eq!(form.logical, expected.logical);
            assert_eq!(form.local_vertices.as_ref(), expected.local_vertices.as_ref());
            assert_eq!(form.surface.mesh.positions, expected.surface.mesh.positions);
            assert_eq!(form.surface.mesh.normals, expected.surface.mesh.normals);
            assert_eq!(form.surface.vertices, expected.surface.vertices);
            assert_eq!(form.source_revision(), (before_stamp.0, before_stamp.1.wrapping_add(1)));
            assert_eq!(form.undo.len(), old_undo + 1);
            assert_eq!(reply["undo"], old_undo + 1);
            assert_eq!(form.definition().hierarchy.as_ref().unwrap().base, original.hierarchy.as_ref().unwrap().base);
            form.definition().hierarchy.as_ref().unwrap().validated_quad_caps().unwrap();
            crate::doc::forms::convert(&form.definition(), 0.05).unwrap();
            browser.forms_prototype.as_mut().unwrap().history(false).unwrap();
            assert_eq!(browser.forms_prototype.as_ref().unwrap().definition(), original);
            browser.forms_prototype.as_mut().unwrap().history(true).unwrap();
            assert_eq!(browser.forms_prototype.as_ref().unwrap().definition(), expected.definition());
        }
    }

    #[test]
    fn browser_coalesced_cap_targets_keep_staged_metadata_and_one_checkpoint() {
        let (mut session, ids) = capped();
        let definition = Arc::new(session.forms_prototype.as_ref().unwrap().definition());
        let form = session.forms_prototype.as_mut().unwrap();
        form.parts.insert(9, super::super::super::Part { name: "Staged source".into(), definition: definition.clone(), levels: Some(1) });
        form.next_part = 10;
        let mut saved = doc::Document::new("Consumed source fixture");
        let feature = saved
            .add_feature(
                doc::FeatureKind::Form {
                    definition: Box::new(definition.as_ref().clone()),
                    tolerance: "0.05 mm".into(),
                    operation: doc::Operation::NewBody,
                    targets: vec![],
                },
                None,
            )
            .unwrap();
        let consumed = Arc::new(saved.feature(feature).unwrap().clone());
        form.consumed.insert(feature, consumed.clone());
        let source = form.definition();
        let old = form.undo.len();
        let first = params(&session, &ids, 0, true);
        assert!(roundtrip(&mut session, first).unwrap().0);
        let second = params(&session, &ids, 1, false);
        assert!(roundtrip(&mut session, second).unwrap().0);
        let form = session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.undo.len(), old + 1);
        assert_eq!(form.next_part, 10);
        assert_eq!(form.parts[&9].levels, Some(1));
        assert!(Arc::ptr_eq(&form.parts[&9].definition, &definition));
        assert!(Arc::ptr_eq(&form.consumed[&feature], &consumed));
        session.forms_prototype.as_mut().unwrap().history(false).unwrap();
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), source);
    }

    #[test]
    fn browser_noop_refusal_and_stale_results_never_consume_local_history() {
        let (mut session, ids) = capped();
        let before = session.forms_prototype.as_ref().unwrap().inspect();
        let no_op = params(&session, &ids, 3, true);
        assert!(!roundtrip(&mut session, no_op).unwrap().0);
        assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
        let given = params(&session, &ids, 0, true);
        let job = session.prepare_form_transform(FormTargetSpace::Surface, given).unwrap();
        let response = BrowserTransform::run(&job.browser_request().unwrap()).unwrap();
        let ready = job.with_browser_reply(&response).unwrap();
        session.active_component = 1234;
        assert!(session.accept_form_transform(ready).is_err());
        assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
        let bad = json!({"vertices":[ids[0]],"positions":[[1e300,0,0]],"checkpoint":true});
        assert!(roundtrip(&mut session, bad).is_err());
        assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
    }

    #[test]
    fn browser_nonpartitioned_surface_keeps_its_authoritative_unowned_display_tag() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":8,"width":8,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let state = session.model.state();
        let support = state.body("Support").unwrap();
        let top = (0..support.body.face_count()).find(|&face| support.body.planar_face(face).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
        session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[2,2],"continuity":0,"partition":false})).unwrap();
        let form = session.forms_prototype.as_ref().unwrap();
        assert!(!form.profile.as_ref().unwrap().partition);
        assert!(form.surface.mesh.tri_face.iter().all(|&face| face == u32::MAX));
        let boundary = form.profile.as_ref().unwrap().boundary();
        let vertex = (0..form.surface.vertices.len()).find(|id| !boundary.contains(id)).unwrap();
        let p = form.surface.vertices[vertex] + Vec3::Z * 0.04;
        let given = json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]],"checkpoint":true});
        let mut native = session.scratch();
        let ready = native.prepare_form_transform(FormTargetSpace::Surface, given.clone()).unwrap().compute().unwrap();
        native.accept_form_transform(ready).unwrap();
        assert!(roundtrip(&mut session, given).unwrap().0);
        let form = session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.definition(), native.forms_prototype.as_ref().unwrap().definition());
        assert_eq!(form.surface.mesh.tri_face, native.forms_prototype.as_ref().unwrap().surface.mesh.tri_face);
        assert!(form.surface.mesh.tri_face.iter().all(|&face| face == u32::MAX));
        assert_eq!(form.inspect()["undo"], native.forms_prototype.as_ref().unwrap().inspect()["undo"]);
    }

    #[test]
    fn browser_equivalent_reordered_tessellation_keeps_exact_model_adoption() {
        let (mut session, ids) = capped();
        let given = params(&session, &ids, 0, true);
        let job = session.prepare_form_transform(FormTargetSpace::Surface, given).unwrap();
        let request = job.browser_request().unwrap();
        let response = BrowserTransform::run(&request).unwrap();
        let mut output: Output = serde_json::from_slice(&response).unwrap();
        let data = output.outcome.as_mut().unwrap().materialized.as_mut().unwrap();
        let expected = data.definition.clone();
        let faces = data.logical.faces.clone();
        let original_triangles = data.surface.mesh.triangles.clone();
        data.surface.mesh.triangles.rotate_left(1);
        data.surface.mesh.tri_face.rotate_left(1);
        assert_ne!(data.surface.mesh.triangles, original_triangles);
        // A different valid display ordering has its own corresponding index.
        // Only this test builds it; page adoption solely hydrates transferred data.
        data.surface.index = MeshIndex::new(&data.surface.mesh).to_wire();
        let reply = encode(&output, BrowserTransform::MAX_REPLY).unwrap();
        let prepared = job.with_browser_reply(&reply).unwrap();
        assert!(session.accept_form_transform(prepared).unwrap().0);
        let accepted = session.forms_prototype.as_ref().unwrap();
        assert_eq!(accepted.definition(), expected);
        assert_eq!(accepted.logical.faces, faces);
        let mut restored = accepted.surface.mesh.triangles.clone();
        restored.rotate_right(1);
        assert_eq!(restored, original_triangles);
        assert!(accepted.hierarchy.as_ref().unwrap().validated_quad_cap().unwrap().is_some());
    }

    #[test]
    fn browser_reply_identity_topology_and_display_corruption_are_atomic() {
        let (session, ids) = capped();
        let given = params(&session, &ids, 0, true);
        let request = session.prepare_form_transform(FormTargetSpace::Surface, given.clone()).unwrap().browser_request().unwrap();
        let response = BrowserTransform::run(&request).unwrap();
        let before = session.forms_prototype.as_ref().unwrap().inspect();
        let original: Value = serde_json::from_slice(&response).unwrap();
        let face = session.forms_prototype.as_ref().unwrap().logical.faces.iter().position(|face| !face.is_empty()).unwrap();
        assert!(!original["outcome"]["Ok"]["materialized"]["logical"]["faces"][face].as_array().unwrap().is_empty());
        for corruption in 0..5 {
            let mut value = original.clone();
            match corruption {
                0 => value["input"][0] = json!(original["input"][0].as_u64().unwrap() ^ 1),
                1 => value["outcome"]["Ok"]["materialized"]["logical"]["faces"][face] = json!([]),
                2 => value["outcome"]["Ok"]["materialized"]["surface"]["mesh"]["triangles"][0][0] = json!(u32::MAX),
                3 => value["outcome"]["Ok"]["materialized"]["definition"]["hierarchy"]["quad_cap"]["face"] = json!(0),
                4 => value["outcome"]["Ok"]["materialized"]["surface"]["mesh"]["tri_face"][0] = json!(u32::MAX),
                _ => panic!("unknown corruption"),
            }
            assert_ne!(value, original, "corruption {corruption} must change the reply");
            let bytes = serde_json::to_vec(&value).unwrap();
            let job = session.prepare_form_transform(FormTargetSpace::Surface, given.clone()).unwrap();
            assert!(job.with_browser_reply(&bytes).is_err(), "corruption {corruption}");
            assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
        }
        let other = params(&session, &ids, 1, true);
        assert!(session.prepare_form_transform(FormTargetSpace::Surface, other).unwrap().with_browser_reply(&response).is_err());
        assert!(BrowserTransform::run(&vec![b' '; BrowserTransform::MAX_REQUEST + 1]).is_err());
        assert!(session.prepare_form_transform(FormTargetSpace::Surface, given).unwrap().with_browser_reply(b"{}").is_err());
    }
}
