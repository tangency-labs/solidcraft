//! Wire input for independent Form CAD checks in a browser compute worker.
use super::{check_fit, check_tolerance, error, initial_reply, params};
use crate::{
    Result, Session, doc,
    geom::subdivision::{Cage, hierarchy::Hierarchy},
    schema,
};
use serde::{Deserialize, Serialize};

const MAX_POINTS: usize = 384;
const MAX_FACES: usize = 256;
const MAX_FACE_CORNERS: usize = 16;
const MAX_EDGE_CREASES: usize = 1024;
const MAX_PARAMETER_PATH: usize = 128;
const MAX_REFINED_POINTS: usize = 1536;
const MAX_REFINED_CELLS: usize = 1024;
const MAX_REPLAY_CELLS: usize = 2048;

/// A deliberately small initial slice. Native checks accept the full design
/// context. An independent Form never reads that context when an explicit
/// numeric tolerance is supplied.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCheck {
    definition: doc::forms::Definition,
    staged: usize,
    tolerance: f64,
    param_check: String,
}

type Reply = std::result::Result<params::ValidationReply, String>;

impl BrowserCheck {
    /// Preflight the work before cloning source, writing JSON or posting a
    /// browser message. No design, history, fit or retained model is copied.
    pub fn available(session: &Session) -> bool {
        session.forms_prototype.as_ref().is_some_and(|form| {
            form.pipe.is_none()
                && form.profile.is_none()
                && form.matching.is_none()
                && !form.has_tool_preview()
                && form.repair.is_none()
                && bounded_source(&form.cage, form.hierarchy.as_ref(), &form.parameters, form.imported.as_deref(), form.symmetry.as_ref())
        })
    }

    /// The host must authorize `form.prototype.validate` before calling this
    /// read-only capture. The worker is given no browser storage capability.
    pub fn capture(session: &Session, tolerance: f64) -> Result<Self> {
        let form = session.forms_prototype.as_ref().ok_or_else(|| error("create a Form first"))?;
        if !Self::available(session) {
            return Err(error("this Form needs the desktop CAD check"));
        }
        Ok(Self {
            definition: form.definition(),
            staged: form.parts.len(),
            tolerance: check_tolerance(Some(tolerance))?,
            param_check: session.param_check.name().into(),
        })
    }

    pub const MAX_PAYLOAD: usize = 128 * 1024;

    pub fn encode(&self) -> Result<Vec<u8>> {
        encode(self)
    }

    /// Worker entry: bound bytes before parsing, recheck the source subset,
    /// and invoke the same command schema and fit/validity helpers as Input.
    pub fn run(bytes: &[u8]) -> Result<Vec<u8>> {
        let outcome = Self::decode(bytes).and_then(|input| input.check()).map_err(|error| error.to_string());
        encode(&outcome)
    }

    pub fn decode_reply(bytes: &[u8]) -> Reply {
        check_length(bytes).map_err(|error| error.to_string())?;
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        check_length(bytes)?;
        serde_json::from_slice(bytes).map_err(|e| error(&e.to_string()))
    }

    fn check(self) -> Result<params::ValidationReply> {
        let source = &self.definition;
        if source.pipe.is_some()
            || source.profile.is_some()
            || source.matching.is_some()
            || !bounded_source(&source.cage, source.hierarchy.as_ref(), &source.parameters, source.imported.as_deref(), source.symmetry.as_ref())
        {
            return Err(error("this Form needs the desktop CAD check"));
        }
        let level = schema::ParamCheck::parse(&self.param_check).ok_or_else(|| error("invalid Form CAD check parameter mode"))?;
        let params = serde_json::json!({ "tolerance": self.tolerance });
        let value = schema::run_read_query_checked(level, &params, "form.prototype.validate", || {
            let tolerance = check_tolerance(Some(self.tolerance))?;
            Ok(serde_json::json!(check_fit(initial_reply(tolerance, self.staged), source, None)))
        })?;
        serde_json::from_value(value).map_err(|e| error(&e.to_string()))
    }
}

fn bounded_source(
    cage: &Cage,
    hierarchy: Option<&Hierarchy>,
    params: &crate::geom::subdivision::primitives::Parameters,
    imported: Option<&doc::forms::Imported>,
    symmetry: Option<&crate::geom::subdivision::symmetry::Symmetry>,
) -> bool {
    let max_points = if hierarchy.is_some() { MAX_REFINED_POINTS } else { MAX_POINTS };
    let topology_fits = match hierarchy {
        Some(hierarchy) => {
            bounded_cage(&hierarchy.base, MAX_POINTS, MAX_FACES)
                && hierarchy.leaves.len() <= MAX_REFINED_CELLS
                && hierarchy.controls.len() <= MAX_REFINED_POINTS
                && hierarchy.replay_face_bound().is_some_and(|bound| bound <= MAX_REPLAY_CELLS)
                && bounded_cage(cage, max_points, MAX_REFINED_CELLS)
        }
        None => bounded_cage(cage, max_points, MAX_FACES),
    };
    topology_fits
        && params.kind.len() <= 16
        && params.path.len() <= MAX_PARAMETER_PATH
        && imported.is_none_or(|source| source.file.len() <= 4096 && source.object.len() <= 256)
        && symmetry.is_none_or(|symmetry| symmetry.partners.len() <= max_points)
}

fn bounded_cage(cage: &Cage, max_points: usize, max_faces: usize) -> bool {
    cage.points.len() <= max_points
        && cage.faces.len() <= max_faces
        && cage.faces.iter().all(|face| face.len() <= MAX_FACE_CORNERS)
        && cage.edge_creases.len() <= MAX_EDGE_CREASES
        && cage.vertex_creases.len() <= max_points
}

fn check_length(bytes: &[u8]) -> Result<()> {
    if bytes.len() > BrowserCheck::MAX_PAYLOAD {
        return Err(error("Form CAD check exceeds the browser transfer limit"));
    }
    Ok(())
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    struct Buffer(Vec<u8>);
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > BrowserCheck::MAX_PAYLOAD - self.0.len() {
                return Err(std::io::Error::other("Form CAD check exceeds the browser transfer limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer(Vec::new());
    serde_json::to_writer(&mut buffer, value).map_err(|e| error(&e.to_string()))?;
    Ok(buffer.0)
}
