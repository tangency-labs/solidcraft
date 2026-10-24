//! Per-session, transient CAM job protocol.
//!
//! [`CamHost`] owns execution; documents and history never contain these values.
//! The intended production backend is an in-process, permissively licensed toolpath
//! engine (see SolidCraft PR #2819 / the `solidcraft-cam` crate): regions in, toolpaths
//! and G-code out. No GPL/LGPL CAM runtime (including FreeCAD Path) is used.
//! [`FakeCamHost`] is the deterministic stand-in used in tests: submitted jobs complete
//! synchronously with empty [`PathSegment`]s so callers can exercise the full command cycle
//! without a live engine.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::{ManufactureSettings, PathSegment};

// ── Identity ──────────────────────────────────────────────────────────────────

/// Session-scoped CAM job identity. Never persisted in the document.
///
/// `session` is a process-unique counter incremented once per [`Session`] (not per job), so jobs
/// from different documents are always distinguishable. `serial` and `generation` count up within
/// one session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CamJobId {
    pub session: u64,
    pub serial: u64,
    pub generation: u64,
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamErrorCode {
    InvalidRequest,
    LimitExceeded,
    HostUnavailable,
    QueueFull,
    /// The model has no visible bodies to export as STEP.
    EmptyGeometry,
    /// The model is still stale (a live gesture is in progress).
    StaleGeometry,
    PostFailed,
    /// The job identity belongs to another session.
    WrongOwner,
    UnknownJob,
    NotCompleted,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CamJobError {
    pub code: CamErrorCode,
    pub message: String,
}

impl CamJobError {
    pub fn new(code: CamErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for CamJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for CamJobError {}

pub type CamResult<T> = std::result::Result<T, CamJobError>;

// ── Phase / Progress ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CamPhase {
    Queued,
    Planning,
    PostProcessing,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

impl CamPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CamProgress {
    pub phase: CamPhase,
    /// Operations completed within the setup.
    pub done: u32,
    /// Total operations in the setup.
    pub total: u32,
}

// ── Provenance ────────────────────────────────────────────────────────────────

/// Metadata linking the result to the exact snapshot that produced it.
///
/// Callers must present this alongside any G-code they distribute: it proves the toolpath
/// was generated from a specific, unmodified model revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CamProvenance {
    /// The setup name used to produce this toolpath.
    pub setup: String,
    /// The document revision at which the STEP snapshot was taken.
    pub scene_generation: u64,
    /// Hex-encoded SHA-256 of the STEP snapshot bytes.
    pub step_sha256: String,
}

// ── Input ─────────────────────────────────────────────────────────────────────

/// Immutable snapshot extracted at job-start time.
///
/// The STEP bytes are captured once and never change while the job runs, so continued
/// CAD editing does not corrupt an in-flight computation.
#[derive(Clone, Debug)]
pub struct CamJobInput {
    /// The setup name to process.
    pub setup: String,
    /// Validated manufacturing intent captured with the geometry. This contains the selected
    /// setup and any machine/post profiles it references, so the backend never has to read
    /// mutable document state after submission.
    pub settings: Arc<ManufactureSettings>,
    /// STEP export of the model at the moment of submission.
    pub step_bytes: Arc<Vec<u8>>,
    /// The document revision the STEP was taken from.
    pub scene_generation: u64,
    /// Hex-encoded SHA-256 of `step_bytes`, computed by the caller.
    pub step_sha256: String,
}

// ── Result ────────────────────────────────────────────────────────────────────

/// Completed output of a CAM job.
#[derive(Clone, Debug)]
pub struct CamJobResult {
    pub job: CamJobId,
    pub provenance: CamProvenance,
    /// Normalised toolpath segments in the WCS frame (mm, mm/min). All segments have been
    /// validated by [`PathSegment::validate`].
    pub segments: Vec<PathSegment>,
    /// Post-processed G-code text. `None` when the setup has no post profile configured.
    pub post_output: Option<String>,
}

// ── Status ────────────────────────────────────────────────────────────────────

/// Summary metadata returned by `manufacture.result` without allocating the segment bytes.
#[derive(Clone, Debug, Serialize)]
pub struct CamResultInfo {
    pub job: CamJobId,
    /// Number of normalised path segments in the result.
    pub segments: usize,
    pub has_post_output: bool,
    pub provenance: CamProvenance,
}

#[derive(Clone, Debug, Serialize)]
pub struct CamJobStatus {
    pub job: CamJobId,
    pub setup: String,
    pub scene_generation: u64,
    pub progress: CamProgress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CamJobError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<CamResultInfo>,
}

// ── Handle ────────────────────────────────────────────────────────────────────

struct HandleInner {
    id: CamJobId,
    status: CamJobStatus,
    result: Option<Arc<CamJobResult>>,
}

/// A live reference to a running or completed CAM job.  `Clone` is cheap (shared `Arc`).
#[derive(Clone)]
pub struct CamHandle(Arc<Mutex<HandleInner>>);

impl CamHandle {
    pub fn new(id: CamJobId, setup: String, scene_generation: u64, total: u32) -> Self {
        let status = CamJobStatus {
            job: id,
            setup,
            scene_generation,
            progress: CamProgress { phase: CamPhase::Queued, done: 0, total },
            error: None,
            result: None,
        };
        CamHandle(Arc::new(Mutex::new(HandleInner { id, status, result: None })))
    }

    pub fn id(&self) -> CamJobId {
        self.0.lock().map(|g| g.id).unwrap_or_else(|e| e.into_inner().id)
    }

    pub fn status(&self) -> CamResult<CamJobStatus> {
        self.0.lock().map(|g| g.status.clone()).map_err(|_| CamJobError::new(CamErrorCode::Internal, "cam job lock poisoned"))
    }

    /// Request cancellation. Transitions non-terminal jobs to [`CamPhase::Cancelling`] so that
    /// the backend's [`CamBackend::pump`] can finish teardown and confirm cancellation
    /// by calling [`Self::adapter_set_progress`] with [`CamPhase::Cancelled`].
    ///
    /// Callers that must archive the job immediately (e.g. `Jobs::submit` with replace) should
    /// force the phase to `Cancelled` if the returned status is still non-terminal.
    pub fn cancel(&self) -> CamResult<CamJobStatus> {
        let mut g = self.0.lock().map_err(|_| CamJobError::new(CamErrorCode::Internal, "cam job lock poisoned"))?;
        if !g.status.progress.phase.terminal() {
            g.status.progress.phase = CamPhase::Cancelling;
        }
        Ok(g.status.clone())
    }

    /// Returns the full result (segments + post output). `None` while still running.
    pub fn result(&self) -> CamResult<Option<Arc<CamJobResult>>> {
        self.0.lock().map(|g| g.result.clone()).map_err(|_| CamJobError::new(CamErrorCode::Internal, "cam job lock poisoned"))
    }

    /// Mark the job completed with the given result.
    /// Public adapter for external [`CamBackend`] implementations.
    pub fn adapter_complete(&self, result: CamJobResult) {
        self.complete(result)
    }

    /// Mark the job failed.
    /// Public adapter for external [`CamBackend`] implementations.
    pub fn adapter_fail(&self, error: CamJobError) {
        self.fail(error)
    }

    /// Update the job's progress phase and done-count without transitioning to a terminal state.
    /// Ignored if the job is already terminal.
    /// Public adapter for external [`CamBackend`] implementations.
    pub fn adapter_set_progress(&self, phase: CamPhase, done: u32) {
        if let Ok(mut g) = self.0.lock()
            && !g.status.progress.phase.terminal()
        {
            g.status.progress.phase = phase;
            g.status.progress.done = done;
        }
    }

    /// Mark the job completed with the given result.
    pub(crate) fn complete(&self, result: CamJobResult) {
        if let Ok(mut g) = self.0.lock()
            && !g.status.progress.phase.terminal()
        {
            let info = CamResultInfo {
                job: result.job,
                segments: result.segments.len(),
                has_post_output: result.post_output.is_some(),
                provenance: result.provenance.clone(),
            };
            g.status.progress.phase = CamPhase::Completed;
            g.status.progress.done = g.status.progress.total;
            g.status.result = Some(info);
            g.result = Some(Arc::new(result));
        }
    }

    /// Mark the job failed.
    pub(crate) fn fail(&self, error: CamJobError) {
        if let Ok(mut g) = self.0.lock()
            && !g.status.progress.phase.terminal()
        {
            g.status.progress.phase = CamPhase::Failed;
            g.status.error = Some(error);
        }
    }
}

// ── Backend descriptor ─────────────────────────────────────────────────────────

/// Availability of a CAM backend as returned by `manufacture.backends`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
}

/// Description of a CAM backend (metadata only; does not start computation).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CamBackendDescriptor {
    pub id: String,
    pub label: String,
    pub availability: Availability,
    /// Human-readable note (e.g. "CAM engine unavailable").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ── CamBackend trait ──────────────────────────────────────────────────────────

/// A host that can execute CAM jobs. The engine calls this from a `&mut Session` but the
/// trait is `Send + Sync + 'static` so implementations may own worker threads.
pub trait CamBackend: Send + Sync + 'static {
    fn submit(&self, id: CamJobId, input: CamJobInput) -> CamResult<CamHandle>;
    fn descriptor(&self) -> CamBackendDescriptor;
    fn is_available(&self) -> bool;
    /// Drive any deferred async work. Called from the UI tick at most twice per second.
    /// The default is a no-op; in-process engines that compute on a worker should override it.
    fn pump(&self) {}
}

// ── CamHost (wrapper) ─────────────────────────────────────────────────────────

/// Caller-owned handle to a CAM execution host. `Clone` is cheap (shared `Arc`).
#[derive(Clone)]
pub struct CamHost(Arc<dyn CamBackend>);

impl CamHost {
    pub fn new(backend: impl CamBackend) -> Self {
        CamHost(Arc::new(backend))
    }

    pub fn fake() -> Self {
        CamHost::new(FakeCamHost)
    }

    pub fn submit(&self, id: CamJobId, input: CamJobInput) -> CamResult<CamHandle> {
        self.0.submit(id, input)
    }

    pub fn descriptor(&self) -> CamBackendDescriptor {
        self.0.descriptor()
    }

    pub fn is_available(&self) -> bool {
        self.0.is_available()
    }

    /// Drive deferred async work. Delegates to [`CamBackend::pump`].
    /// Called from the UI tick; safe to call at any frequency.
    pub fn pump(&self) {
        self.0.pump();
    }
}

// ── FakeCamHost ───────────────────────────────────────────────────────────────

/// Deterministic fake: jobs complete synchronously with zero path segments and no post output.
///
/// Every submitted job transitions directly from `Queued` to `Completed`.  The STEP bytes are
/// read for their SHA-256 hash (provenance) but are not interpreted.  Use this in unit tests
/// that exercise the full `manufacture.preflight / start / status / result / export` cycle
/// without a live CAM engine.
#[derive(Clone, Debug, Default)]
pub struct FakeCamHost;

impl CamBackend for FakeCamHost {
    fn submit(&self, id: CamJobId, input: CamJobInput) -> CamResult<CamHandle> {
        let handle = CamHandle::new(id, input.setup.clone(), input.scene_generation, 0);
        let result = CamJobResult {
            job: id,
            provenance: CamProvenance { setup: input.setup, scene_generation: input.scene_generation, step_sha256: input.step_sha256 },
            segments: Vec::new(),
            post_output: None,
        };
        handle.complete(result);
        Ok(handle)
    }

    fn descriptor(&self) -> CamBackendDescriptor {
        CamBackendDescriptor { id: "fake-cam".into(), label: "Fake CAM (tests)".into(), availability: Availability::Available, note: None }
    }

    fn is_available(&self) -> bool {
        true
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_input(setup: &str) -> CamJobInput {
        let step = b"ISO-10303-21;\nDATA;\nENDSEC;\nEND-ISO-10303-21;".to_vec();
        let sha = format!("{:x}", {
            let mut h = 0u64;
            for b in &step {
                h = h.wrapping_mul(31).wrapping_add(*b as u64);
            }
            h
        });
        CamJobInput {
            setup: setup.into(),
            settings: Arc::new(ManufactureSettings::default()),
            step_bytes: Arc::new(step),
            scene_generation: 1,
            step_sha256: sha,
        }
    }

    fn dummy_id(session: u64, serial: u64) -> CamJobId {
        CamJobId { session, serial, generation: serial }
    }

    #[test]
    fn fake_host_completes_immediately() {
        let host = CamHost::fake();
        assert!(host.is_available());
        let id = dummy_id(1, 1);
        let handle = host.submit(id, dummy_input("Setup1")).unwrap();
        let status = handle.status().unwrap();
        assert_eq!(status.progress.phase, CamPhase::Completed, "fake job must complete synchronously");
        assert_eq!(status.job, id);
        let result = handle.result().unwrap().unwrap();
        assert_eq!(result.provenance.setup, "Setup1");
        assert!(result.segments.is_empty());
        assert!(result.post_output.is_none());
    }

    #[test]
    fn cancel_completed_job_is_idempotent() {
        let host = CamHost::fake();
        let id = dummy_id(2, 1);
        let handle = host.submit(id, dummy_input("S")).unwrap();
        // Already completed; cancel must leave it in Completed.
        let after = handle.cancel().unwrap();
        assert_eq!(after.progress.phase, CamPhase::Completed);
    }

    #[test]
    fn id_roundtrips_json() {
        let id = CamJobId { session: 7, serial: 3, generation: 5 };
        let json = serde_json::to_string(&id).unwrap();
        let back: CamJobId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn descriptor_is_available() {
        let desc = CamHost::fake().descriptor();
        assert_eq!(desc.availability, Availability::Available);
        assert!(!desc.id.is_empty());
    }
}
