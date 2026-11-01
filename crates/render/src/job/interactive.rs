//! Transient CPU Interactive scheduling. The UI adapter owns visual revisions;
//! this controller owns cancellation, not document settings or final-image export.
use std::time::Duration;

use super::{CpuProgressiveOptions, ErrorCode, ImageRequest, Integrator, JobError, JobHandle, JobId, Phase, Result};

const SETTLE: Duration = Duration::from_millis(200);
const MAX_EDGE: u32 = 1024;

/// Prepare a separate low-resolution request. Never edits the final request,
/// changes renderer, removes optics/lights, or promises geometry admission.
pub fn cpu_request(final_request: &ImageRequest) -> Result<ImageRequest> {
    if final_request.integrator != Integrator::Raytrace || final_request.transparent_background {
        return Err(JobError::new(ErrorCode::InvalidRequest, "Interactive CPU requires opaque ray tracing"));
    }
    let (width, height) = (final_request.width, final_request.height);
    if width == 0 || height == 0 {
        return Err(JobError::new(ErrorCode::InvalidRequest, "Interactive output dimensions must be positive"));
    }
    if let Some(options) = &final_request.raytrace {
        options.validate()?;
    }
    let edge = width.max(height);
    let fit = |side: u32| {
        if edge <= MAX_EDGE { side } else { ((u64::from(side) * u64::from(MAX_EDGE) + u64::from(edge) / 2) / u64::from(edge)).max(1) as u32 }
    };
    let mut request = final_request.clone();
    request.width = fit(width);
    request.height = fit(height);
    let options = request.raytrace.get_or_insert_default();
    options.primary_samples = 1;
    // These are preview sampling choices only. Incompatible transport remains an
    // explicit error, rather than silently removing the user's requested effects.
    options.progressive = Some(CpuProgressiveOptions::default());
    if let Some(reflections) = &mut options.reflections {
        reflections.samples = 1;
    }
    options.validate()?;
    options.budget.validate_image(request.width, request.height, options.progressive)?;
    Ok(request)
}

/// Caller increments visual_revision for every camera/draft/appearance/light/HDR
/// or output edit, even if scene_generation did not change. Never use only a CAD
/// revision, a body name, or the selected setup name as the visual identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualRevision {
    pub scene_generation: u64,
    pub visual_revision: u64,
}

/// Issued only after the edit group settles. Private fields prevent fabricating
/// a ticket for a newer request from a delayed capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureTicket {
    session: u64,
    serial: u64,
    revision: VisualRevision,
}

impl CaptureTicket {
    pub fn revision(self) -> VisualRevision {
        self.revision
    }

    /// Submit this identity to the preview host, outside the final-job registry.
    pub fn job_id(self) -> JobId {
        JobId { session: self.session, serial: self.serial, generation: self.serial }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    Updating,
    Refining,
    Paused,
    TargetReached,
    Error,
}

/// One controller per document session. No serialization, clocks, threads, scene
/// extraction or new renderer here. Supply monotonic elapsed time from the host;
/// this keeps both browser integration and deterministic event tests possible.
pub struct Interactive {
    session: u64,
    serial: u64,
    revision: Option<VisualRevision>,
    changed_at: Duration,
    enabled: bool,
    paused: bool,
    final_active: bool,
    ticket: Option<CaptureTicket>,
    active: Option<JobHandle>,
    error: Option<JobError>,
}

impl Interactive {
    pub fn new(session: u64) -> Self {
        Self {
            session,
            serial: 0,
            revision: None,
            changed_at: Duration::ZERO,
            enabled: false,
            paused: false,
            final_active: false,
            ticket: None,
            active: None,
            error: None,
        }
    }

    fn cancel(&mut self) -> Result<()> {
        // Invalidate first, including when a poisoned host refuses cancellation.
        self.ticket = None;
        if let Some(job) = self.active.take() {
            job.cancel()?;
        }
        Ok(())
    }

    /// Explicit user activation. Merely entering Render must not call this.
    pub fn enable(&mut self, revision: VisualRevision, now: Duration) -> Result<()> {
        self.cancel()?;
        self.enabled = true;
        self.paused = false;
        self.error = None;
        self.revision = Some(revision);
        self.changed_at = now;
        Ok(())
    }

    /// Workspace/document departure uses disable; manually started final jobs
    /// are deliberately outside this controller and are never cancelled here.
    pub fn disable(&mut self) -> Result<()> {
        self.enabled = false;
        self.paused = false;
        self.error = None;
        self.cancel()
    }

    pub fn edit(&mut self, revision: VisualRevision, now: Duration) -> Result<()> {
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            self.changed_at = now;
            self.error = None;
            self.cancel()?;
        }
        Ok(())
    }

    /// A view may retain an identified immutable PNG before pausing, but must
    /// label it paused/stale. It is never a current or exportable final image.
    pub fn pause(&mut self) -> Result<()> {
        self.paused = true;
        self.cancel()
    }

    pub fn resume(&mut self, now: Duration) -> Result<()> {
        self.cancel()?;
        self.paused = false;
        self.error = None;
        self.changed_at = now;
        Ok(())
    }

    pub fn set_final_active(&mut self, active: bool, now: Duration) -> Result<()> {
        if self.final_active != active {
            self.final_active = active;
            self.changed_at = now;
            self.cancel()?;
        }
        Ok(())
    }

    /// At most one unacknowledged capture; no queue of obsolete requests.
    pub fn capture(&mut self, now: Duration) -> Result<Option<CaptureTicket>> {
        if !self.enabled
            || self.paused
            || self.final_active
            || self.ticket.is_some()
            || self.error.is_some()
            || now.checked_sub(self.changed_at).is_none_or(|elapsed| elapsed < SETTLE)
        {
            return Ok(None);
        }
        let Some(revision) = self.revision else { return Ok(None) };
        self.serial = self.serial.checked_add(1).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "Interactive capture identity exhausted"))?;
        let ticket = CaptureTicket { session: self.session, serial: self.serial, revision };
        self.ticket = Some(ticket);
        Ok(Some(ticket))
    }

    /// Attach an admitted preview-only job after immutable capture. Delayed jobs
    /// are cancelled, including a previous ticket for the same visual revision.
    /// Use a separate engine job session/registry from final-image export.
    pub fn attach(&mut self, ticket: CaptureTicket, job: JobHandle) -> Result<bool> {
        // A misrouted callback must never cancel another document's work or a
        // manual final job. Only our dedicated preview session is ours to stop.
        if job.id().session != self.session {
            self.capture_failed(ticket, JobError::new(ErrorCode::WrongOwner, "Interactive job belongs to another session"));
            return Ok(false);
        }
        if self.ticket != Some(ticket) || ticket.session != self.session {
            if self.active.as_ref().is_none_or(|active| active.id() != job.id()) {
                job.cancel()?;
            }
            return Ok(false);
        }
        let status = job.status()?;
        if status.job != ticket.job_id() || status.scene_generation != ticket.revision.scene_generation {
            job.cancel()?;
            self.capture_failed(ticket, JobError::new(ErrorCode::WrongOwner, "Interactive job capture identity mismatch"));
            return Ok(false);
        }
        if status.request.integrator != Integrator::Raytrace || status.request.raytrace.as_ref().is_none_or(|options| options.progressive.is_none()) {
            job.cancel()?;
            self.capture_failed(ticket, JobError::new(ErrorCode::InvalidRequest, "Interactive requires a CPU progressive job"));
            return Ok(false);
        }
        // A ticket is acknowledged exactly once, even if the same capture is
        // delivered twice while an earlier attached job is still running.
        if self.active.is_some() {
            if self.active.as_ref().is_none_or(|active| active.id() != job.id()) {
                job.cancel()?;
            }
            return Ok(false);
        }
        self.active = Some(job);
        Ok(true)
    }

    pub fn capture_failed(&mut self, ticket: CaptureTicket, error: JobError) {
        if self.ticket == Some(ticket) && self.active.is_none() {
            self.ticket = None;
            self.error = Some(error);
        }
    }

    /// Access only the current capture. The adapter reads checkpoint/final bytes
    /// through this handle, displaying both as Interactive/provisional content.
    pub fn current(&self) -> Option<&JobHandle> {
        self.active.as_ref()
    }

    pub fn error(&self) -> Option<&JobError> {
        self.error.as_ref()
    }

    pub fn state(&self) -> Result<State> {
        if !self.enabled {
            return Ok(State::Off);
        }
        if self.paused || self.final_active {
            return Ok(State::Paused);
        }
        if self.error.is_some() {
            return Ok(State::Error);
        }
        if let Some(job) = &self.active {
            return Ok(match job.status()?.progress.phase {
                Phase::Completed => State::TargetReached,
                Phase::Cancelled | Phase::Failed => State::Error,
                _ => State::Refining,
            });
        }
        Ok(State::Updating)
    }
}

impl Drop for Interactive {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}
