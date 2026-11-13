//! Read-only native face picking on a qualified accepted Form snapshot.
use super::{Element, context, live_cad};
use crate::SolveApp;
use solidcore_record::Vec3;
use solidcore_record::brep::Brep;
use solidcraft_engine::geom::brep::ray;
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

const PREPARE_WORK: usize = 8_000_000;
const RAY_WORK: usize = 8_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Face(usize),
    Surface,
}

pub type Answer = ray::Front<Owner>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    FormNotReady,
    ContextNotReady,
    ContextWithoutCad,
    GeometryBudget,
    Allocation,
}

/// Cheap source capture for a worker. Callers qualify their camera and input
/// separately before using the result of a viewport gesture.
pub struct Query {
    exact: Arc<live_cad::ExactSelection>,
    allowed: Arc<BTreeSet<Element>>,
    context: RetainedContext,
    prepared: OnceLock<Prepared>,
}

struct RetainedContext {
    version: u64,
    records: Vec<Arc<Brep>>,
}

impl RetainedContext {
    fn capture(app: &SolveApp) -> Result<Self, CaptureError> {
        if !context::complete(app) {
            return Err(CaptureError::ContextNotReady);
        }
        let mut records = Vec::new();
        for entry in context::retained_entries(app) {
            if records.len() >= ray::MAX_CONTEXT {
                return Err(CaptureError::GeometryBudget);
            }
            let body = entry.exact.as_ref().ok_or(CaptureError::ContextWithoutCad)?;
            records.try_reserve(1).map_err(|_| CaptureError::Allocation)?;
            records.push(body.shared_record());
        }
        Ok(Self { version: app.forms_prototype.context.version, records })
    }

    fn is_current(&self, app: &SolveApp) -> bool {
        if self.version != app.forms_prototype.context.version || !context::complete(app) {
            return false;
        }
        let mut before = self.records.iter();
        for entry in context::retained_entries(app) {
            let (Some(old), Some(new)) = (before.next(), &entry.exact) else { return false };
            if !std::ptr::eq(old.as_ref(), new.record()) {
                return false;
            }
        }
        before.next().is_none()
    }
}

impl Query {
    pub fn capture(app: &SolveApp) -> Result<Self, CaptureError> {
        if !usable_form(app) {
            return Err(CaptureError::FormNotReady);
        }
        let exact = live_cad::selection_geometry(app).ok_or(CaptureError::FormNotReady)?;
        let allowed = live_cad::selection_allowed(app).ok_or(CaptureError::FormNotReady)?;
        if exact.faces.len() > ray::MAX_FACES {
            return Err(CaptureError::GeometryBudget);
        }
        Ok(Self { exact: exact.clone(), allowed: allowed.clone(), context: RetainedContext::capture(app)?, prepared: OnceLock::new() })
    }

    /// Checks the accepted CAD source and the opaque context, including the
    /// retained set when Boolean preview changes which bodies are displayed.
    pub fn is_current(&self, app: &SolveApp) -> bool {
        usable_form(app)
            && live_cad::selection_geometry(app).is_some_and(|current| Arc::ptr_eq(&self.exact, current))
            && live_cad::selection_allowed(app).is_some_and(|allowed| Arc::ptr_eq(&self.allowed, allowed))
            && self.context.is_current(app)
    }

    /// Build native bounds and map allowed owners on the worker. Reuse the
    /// prepared scene for this accepted snapshot and retry failed preparation.
    pub fn prepare(&self) -> Result<&Prepared, ray::FrontError> {
        if let Some(prepared) = self.prepared.get() {
            return Ok(prepared);
        }
        let mut owners = Vec::new();
        owners.try_reserve_exact(self.exact.faces.len()).map_err(|_| ray::ProofError::Allocation)?;
        owners.extend(self.exact.faces.iter().map(|face| match face {
            live_cad::ExactFaceOwner::Face(id) if self.allowed.contains(&Element::Face(*id)) => Some(Owner::Face(*id)),
            live_cad::ExactFaceOwner::Surface if self.allowed.contains(&Element::Surface) => Some(Owner::Surface),
            _ => None,
        }));
        let scene = ray::Scene::prepare(self.exact.body.shared_record(), &owners, &self.context.records, &mut { PREPARE_WORK })?;
        Ok(self.prepared.get_or_init(|| Prepared { scene }))
    }
}

fn usable_form(app: &SolveApp) -> bool {
    app.session.forms_prototype.as_ref().is_some_and(|form| form.repair.is_none() && !form.has_tool_preview())
}

/// Prepared bounds can be used for repeated rays on the same snapshot.
pub struct Prepared {
    scene: ray::Scene<Arc<Brep>, Owner>,
}

impl Prepared {
    pub fn front(&self, origin: Vec3, direction: Vec3, range: [f64; 2]) -> Result<Answer, ray::FrontError> {
        self.scene.front([origin.x, origin.y, origin.z], [direction.x, direction.y, direction.z], range, &mut { RAY_WORK })
    }
}
