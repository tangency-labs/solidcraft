//! Native UI preparation of the same strict Check/Start commands beside the
//! window. No job starts in the scratch owner; commit uses the real registry.
pub mod batch;
pub(crate) mod batch_payload;

use crate::{Result, Session, cmd::PreparedCapture};
use serde_json::{Value, json};
use solidcraft_render::job::CancelToken;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Check,
    Start,
}
impl Kind {
    fn command(self) -> &'static str {
        match self {
            Self::Check => "render.preflight",
            Self::Start => "render.start",
        }
    }
}

/// Move-only, fully proved capture. The already-created scratch owner provides
/// exact source provenance without a second CAD snapshot or a cache-pointer guess.
/// Renderer resources/leases drop before that temporary CAD owner.
pub struct Prepared {
    capture: PreparedCapture,
    source: Session,
    kind: Kind,
    shot: Option<String>,
    params: Value,
    completed: bool,
}
impl Prepared {
    pub fn kind(&self) -> Kind {
        self.kind
    }
}

/// Consumes Session::scratch, not the live window Session. The caller carries its
/// VFS scope and cancellation token onto its native preparation worker.
pub fn prepare(snapshot: Session, kind: Kind, params: Value, cancel: CancelToken) -> Result<Prepared> {
    prepare_with_metadata(snapshot, kind, params, cancel, None)
}
fn prepare_with_metadata(
    snapshot: Session,
    kind: Kind,
    params: Value,
    cancel: CancelToken,
    retained: Option<&solidcraft_render::job::RetainedMetadata>,
) -> Result<Prepared> {
    cancel.check().map_err(crate::render_jobs::job_error)?;
    let params = if params.is_null() { Value::Object(Default::default()) } else { params };
    validate_command(&snapshot, kind, &params)?;
    let shot = params.get("setup").and_then(Value::as_str).map(str::to_owned);
    let capture = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::cmd::prepare_capture_with_metadata(&snapshot, &params, kind.command(), &cancel, retained)
    })) {
        Ok(result) => result?,
        Err(cause) => {
            let message =
                cause.downcast_ref::<&str>().map(|value| value.to_string()).or_else(|| cause.downcast_ref::<String>().cloned()).unwrap_or_default();
            return Err(crate::EngineError::Internal(kind.command().into(), message));
        }
    };
    submission(&capture)?.check_cancel().map_err(crate::render_jobs::job_error)?;
    Ok(Prepared { capture, source: snapshot, kind, shot, params, completed: false })
}

// Keep the existing engine command enablement and embedding-host refusal.
// UI/control schema Off/Warn/Strict remains its ordinary dispatch policy.
fn validate_command(session: &Session, kind: Kind, params: &Value) -> Result<()> {
    validate_command_id(session, kind.command(), params)
}
fn validate_command_id(session: &Session, id: &str, params: &Value) -> Result<()> {
    let spec = crate::find_command(id).ok_or_else(|| crate::EngineError::UnknownCommand(id.into()))?;
    (spec.enabled)(session).map_err(|why| crate::EngineError::Disabled(spec.id.into(), why))?;
    session.check_command(spec, params)
}

fn submission(capture: &PreparedCapture) -> Result<&solidcraft_render::job::PreparedSubmission> {
    capture.submission.as_ref().ok_or_else(|| crate::EngineError::Other("prepared render was already consumed".into()))
}

fn same_moves(a: &BTreeMap<u64, solidcraft_doc::Mat>, b: &BTreeMap<u64, solidcraft_doc::Mat>) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|((ka, a), (kb, b))| ka == kb && a.iter().flatten().zip(b.iter().flatten()).all(|(a, b)| a.to_bits() == b.to_bits()))
}
fn camera_bits(camera: Option<solidcraft_render::Camera>) -> Option<(solidcraft_render::UpAxis, [u64; 8])> {
    camera.map(|camera| {
        (
            camera.up,
            [camera.target.x, camera.target.y, camera.target.z, camera.yaw, camera.pitch, camera.roll, camera.distance, camera.fov].map(f64::to_bits),
        )
    })
}
fn current(source: &Session, session: &Session) -> bool {
    source.uid == session.uid
        && source.revision == session.revision
        && Arc::ptr_eq(&source.doc, &session.doc)
        && Arc::ptr_eq(&source.model.state(), &session.model.state())
        && !session.model.is_stale()
        && source.active_sketch == session.active_sketch
        && source.feature_edit_sketch == session.feature_edit_sketch
        && source.active_component == session.active_component
        && source.active_occurrence_path == session.active_occurrence_path
        && source.visibility == session.visibility
        && same_moves(&source.pending_moves, &session.pending_moves)
        && same_moves(&source.exploded, &session.exploded)
        && camera_bits(source.camera) == camera_bits(session.camera)
}

/// Strict synchronous owner wrapper. Native UI completion uses finish_retained
/// and returns the retained owner to its preparation worker for disposal.
pub fn finish(session: &mut Session, mut prepared: Prepared) -> Result<Value> {
    finish_retained(session, &mut prepared)
}

/// Revalidate and commit without destroying the large captured source on the
/// caller. Every error retains the capsule and its leases; success retains its
/// scratch source. The native caller returns this owner to the same worker and
/// waits for retirement acknowledgement before reporting resources released.
pub fn finish_retained(session: &mut Session, prepared: &mut Prepared) -> Result<Value> {
    finish_inner(session, prepared, None)
}
fn finish_inner(session: &mut Session, prepared: &mut Prepared, ticket: Option<batch::Ticket>) -> Result<Value> {
    if prepared.completed {
        return Err(crate::EngineError::Other("prepared render was already completed".into()));
    }
    if !current(&prepared.source, session) {
        return Err(crate::EngineError::Other("render source changed while preparing; check again".into()));
    }
    validate_command(session, prepared.kind, &prepared.params)?;
    let captured = submission(&prepared.capture)?;
    let installed = session
        .render_jobs
        .host_for(captured.executor())
        .ok_or_else(|| crate::EngineError::Other("prepared render host is no longer installed".into()))?;
    if !installed.same_host(captured.host()) || !installed.is_available() {
        return Err(crate::EngineError::Other("prepared render host changed before completion".into()));
    }
    captured.check_cancel().map_err(crate::render_jobs::job_error)?;
    if prepared.kind == Kind::Check {
        prepared.completed = true;
        return Ok(std::mem::take(&mut prepared.capture.reply));
    }
    let status = session
        .render_jobs
        .try_start_prepared_inner(session.revision, &mut prepared.capture.submission, prepared.capture.replace, prepared.shot.clone(), ticket)
        .map_err(crate::render_jobs::job_error)?;
    prepared.completed = true;
    let mut status = json!(status);
    if let (Some(out), Some(ignored)) = (status.as_object_mut(), prepared.capture.reply.get("ignored_controls")) {
        out.insert("ignored_controls".into(), ignored.clone());
    }
    Ok(status)
}
