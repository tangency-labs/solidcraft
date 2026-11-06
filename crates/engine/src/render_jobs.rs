//! Per-document, transient render ownership. Captured scenes remain immutable when CAD edits
//! continue; callers get the captured revision and must never present it as a newer scene.

use solidcraft_render::{
    ExecutorDescriptor,
    job::{CpuHost, ErrorCode, HostKind, Integrator, JobError, JobHandle, JobId, JobStatus, Phase, RenderFrame},
};
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// The native CPU worker has two explicit execution methods. This pure metadata
/// lookup neither creates a host nor enables raytracing in cooperative/feature-off builds.
/// A stopped native host keeps its descriptor, marked unavailable by the host.
pub(crate) fn cpu_raytrace_descriptor(host: Option<&CpuHost>) -> Option<ExecutorDescriptor> {
    let host = host.filter(|host| host.kind() == HostKind::NativeWorker)?;
    host.descriptor_for(Integrator::Raytrace).ok()
}

fn owner() -> u64 {
    let mut n = NEXT_OWNER.load(Ordering::Relaxed);
    loop {
        let Some(next) = n.checked_add(1) else { return 0 };
        match NEXT_OWNER.compare_exchange_weak(n, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return n,
            Err(current) => n = current,
        }
    }
}

struct LiveJob {
    handle: JobHandle,
    shot: Option<String>,
}

/// Transient completed-image metadata. Neither pixels nor selection enter a design or undo.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RetainedResult {
    pub frame: solidcraft_render::job::FrameInfo,
    pub shot: Option<String>,
    pub seconds: f64,
    pub stale: bool,
}

struct Completed {
    handle: JobHandle,
    info: RetainedResult,
}

pub(crate) struct BatchShot {
    pub name: String,
    pub camera: solidcraft_render::Camera,
    pub request: Arc<serde_json::Value>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RenderQueueState {
    pub running: usize,
    pub waiting: usize,
    /// A real native preparation claim, separate from an invented running job.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub preparing: bool,
    pub job: Option<JobId>,
    pub shot: Option<String>,
    pub error: Option<String>,
}

pub(crate) struct Jobs {
    owner: u64,
    serial: u64,
    generation: u64,
    pub(crate) host: Option<CpuHost>,
    pub(crate) descriptor: Option<ExecutorDescriptor>,
    pub(crate) additional: Vec<(CpuHost, ExecutorDescriptor)>,
    live: Vec<LiveJob>,
    history: VecDeque<JobStatus>,
    completed: VecDeque<Completed>,
    selected: Option<solidcraft_render::job::JobId>,
    waiting: VecDeque<BatchShot>,
    pub(crate) batch_job: Option<JobId>,
    pub(crate) batch_error: Option<String>,
    pub(crate) native_batch: Option<crate::render_preparation::batch::NativeState>,
    pub(crate) native_epoch: u64,
    comparison: Option<[Option<JobId>; 2]>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self {
            owner: owner(),
            serial: 0,
            generation: 0,
            host: None,
            descriptor: None,
            additional: Vec::new(),
            live: Vec::new(),
            history: VecDeque::new(),
            completed: VecDeque::new(),
            selected: None,
            waiting: VecDeque::new(),
            batch_job: None,
            batch_error: None,
            native_batch: None,
            native_epoch: 0,
            comparison: None,
        }
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        if let Some(native) = &self.native_batch {
            native.cancel.cancel();
        }
        for job in &self.live {
            let _ = job.handle.cancel();
        }
    }
}

impl Jobs {
    pub(crate) fn owner_key(&self) -> u64 {
        self.owner
    }
    pub(crate) fn set_comparison(&mut self, sides: Option<[Option<JobId>; 2]>) {
        self.comparison = sides;
        if let Some([left, right]) = sides {
            self.selected = right.or(left);
        }
    }

    fn valid_comparison(&self) -> Option<[Option<JobId>; 2]> {
        self.comparison.filter(|sides| sides.iter().flatten().all(|job| self.completed.iter().any(|result| result.handle.id() == *job)))
    }
    pub(crate) fn has_results(&self) -> bool {
        !self.completed.is_empty() || self.live.iter().any(|job| job.handle.frame().ok().flatten().is_some())
    }

    pub(crate) fn host_for(&self, executor: &solidcraft_render::ExecutorId) -> Option<&CpuHost> {
        if executor.as_str() == "cpu-raytrace" {
            return self
                .host
                .as_ref()
                .filter(|host| host.descriptor_for(solidcraft_render::job::Integrator::Raytrace).is_ok_and(|d| &d.id == executor));
        }
        if self.descriptor.as_ref().is_some_and(|d| &d.id == executor) {
            return self.host.as_ref();
        }
        self.additional.iter().find(|(_, d)| &d.id == executor).map(|(host, _)| host)
    }

    pub(crate) fn scratch(&self) -> Self {
        let mut jobs = Self::default();
        jobs.host = self.host.clone();
        jobs.descriptor = self.descriptor.clone();
        jobs.additional = self.additional.clone();
        jobs
    }

    pub(crate) fn collect(&mut self) -> Result<(), JobError> {
        let mut i = 0;
        while i < self.live.len() {
            let status = self.live.get(i).ok_or_else(|| JobError::new(ErrorCode::Internal, "missing live job"))?.handle.status()?;
            if !status.progress.phase.terminal() {
                i += 1;
                continue;
            }
            let job = self.live.remove(i);
            if let Some(frame) = status.frame.as_ref().filter(|_| status.progress.phase == Phase::Completed) {
                let seconds = job.handle.completed_duration()?.map_or(0.0, |duration| duration.as_secs_f64());
                let completed_id = job.handle.id();
                self.completed.push_back(Completed {
                    handle: job.handle,
                    info: RetainedResult { frame: frame.clone(), shot: job.shot, seconds, stale: false },
                });
                while self.completed.len() > 8 {
                    self.completed.pop_front();
                }
                if self.valid_comparison().is_none() {
                    self.comparison = None;
                    self.selected = Some(completed_id);
                }
            }
            self.history.push_back(status);
            while self.history.len() > 4 {
                self.history.pop_front();
            }
        }
        Ok(())
    }

    pub(crate) fn start_prepared(
        &mut self,
        revision: u64,
        prepared: solidcraft_render::job::PreparedSubmission,
        replace: bool,
        shot: Option<String>,
    ) -> Result<JobStatus, JobError> {
        self.try_start_prepared(revision, &mut Some(prepared), replace, shot)
    }

    pub(crate) fn try_start_prepared(
        &mut self,
        revision: u64,
        slot: &mut Option<solidcraft_render::job::PreparedSubmission>,
        replace: bool,
        shot: Option<String>,
    ) -> Result<JobStatus, JobError> {
        self.try_start_prepared_inner(revision, slot, replace, shot, None)
    }
    pub(crate) fn try_start_prepared_inner(
        &mut self,
        revision: u64,
        slot: &mut Option<solidcraft_render::job::PreparedSubmission>,
        replace: bool,
        shot: Option<String>,
        ticket: Option<crate::render_preparation::batch::Ticket>,
    ) -> Result<JobStatus, JobError> {
        let prepared = slot.as_ref().ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "prepared render was already consumed"))?;
        let batch_next = if let Some(ticket) = ticket {
            let native = self
                .native_batch
                .as_ref()
                .filter(|n| n.ticket == ticket && n.claimed && !n.cancel.is_cancelled())
                .ok_or_else(|| JobError::new(ErrorCode::Cancelled, "native shot queue claim is no longer current"))?;
            if replace || native.next >= native.total {
                return Err(JobError::new(ErrorCode::InvalidRequest, "invalid native shot queue front"));
            }
            Some(native.next + 1)
        } else {
            None
        };
        if ticket.is_none() && (self.batch_job.is_some() || self.native_batch.is_some()) {
            if !replace {
                return Err(JobError::new(ErrorCode::QueueFull, "cancel the shot queue before starting another render"));
            }
            self.waiting.clear();
            self.batch_job = None;
            if let Some(native) = self.native_batch.take() {
                native.cancel.cancel();
            }
        }
        self.collect()?;
        let host = self
            .host_for(prepared.executor())
            .ok_or_else(|| JobError::new(ErrorCode::HostUnavailable, "resolved image host is not installed"))?
            .clone();
        if !host.same_host(prepared.host()) {
            return Err(JobError::new(ErrorCode::WrongOwner, "prepared render host changed before enqueue"));
        }
        prepared.check_cancel()?;
        if replace {
            for job in self.live.drain(..) {
                let mut status = job.handle.cancel()?;
                // Replaced handles are deliberately abandoned: keep a cancellation tombstone,
                // not a permanently "cancelling" snapshot. The token already prevents any
                // publication; the host can finish releasing its buffers asynchronously.
                if !status.progress.phase.terminal() {
                    status.progress.phase = Phase::Cancelled;
                }
                self.history.push_back(status);
            }
            while self.history.len() > 4 {
                self.history.pop_front();
            }
        }
        if self.live.len() >= 2 {
            return Err(JobError::new(ErrorCode::QueueFull, "this document already has an active and pending render"));
        }
        if self.owner == 0 {
            return Err(JobError::new(ErrorCode::LimitExceeded, "render session identity exhausted"));
        }
        let serial = self.serial.checked_add(1).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "render job identity exhausted"))?;
        let generation = self.generation.checked_add(1).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "render generation exhausted"))?;
        let handle = host.try_submit_prepared(JobId { session: self.owner, serial, generation }, revision, slot)?;
        let status = handle.status()?;
        self.serial = serial;
        self.generation = generation;
        self.live.push(LiveJob { handle, shot });
        if let Some(next) = batch_next {
            if let Some(native) = &mut self.native_batch {
                native.next = next;
                native.claimed = false;
            }
            self.batch_job = Some(status.job);
        }
        Ok(status)
    }

    pub(crate) fn regrade(
        &mut self,
        source_id: JobId,
        presentation: solidcraft_render::job::presentation::Presentation,
    ) -> Result<JobStatus, JobError> {
        if self.batch_job.is_some() || self.native_batch.is_some() {
            return Err(JobError::new(ErrorCode::QueueFull, "cancel the shot queue before regrading"));
        }
        self.collect()?;
        let source = self.handle(source_id)?;
        let frame = source.frame()?.ok_or_else(|| JobError::new(ErrorCode::NotCompleted, "regrade requires a completed frame"))?;
        let master = frame.master_source()?;
        // Prefer the source pool so its retained source and derivative work share
        // one admission authority. Regrading itself never invokes its executor.
        let host = self
            .host_for(&master.info.executor)
            .or(self.host.as_ref())
            .ok_or_else(|| JobError::new(ErrorCode::HostUnavailable, "no image host is installed for master presentation"))?
            .clone();
        if self.live.len() >= 2 {
            return Err(JobError::new(ErrorCode::QueueFull, "this document already has an active and pending render"));
        }
        let serial = self.serial.checked_add(1).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "render job identity exhausted"))?;
        let generation = self.generation.checked_add(1).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "render generation exhausted"))?;
        let handle = host.submit_regrade(JobId { session: self.owner, serial, generation }, &source, presentation)?;
        let status = handle.status()?;
        self.serial = serial;
        self.generation = generation;
        let shot = self.completed.iter().find(|result| result.handle.id() == source_id).and_then(|result| result.info.shot.clone());
        self.live.push(LiveJob { handle, shot });
        Ok(status)
    }

    fn check_owner(&self, id: JobId) -> Result<(), JobError> {
        if id.session != self.owner {
            return Err(JobError::new(ErrorCode::WrongOwner, "render job belongs to another document session"));
        }
        Ok(())
    }

    pub(crate) fn handle(&self, id: JobId) -> Result<JobHandle, JobError> {
        self.check_owner(id)?;
        self.live
            .iter()
            .map(|job| &job.handle)
            .chain(self.completed.iter().map(|result| &result.handle))
            .find(|j| j.id() == id)
            .cloned()
            .ok_or_else(|| JobError::new(ErrorCode::UnknownJob, "render job is no longer retained"))
    }

    /// Metadata-only identity validation for deferred native cancellation.
    /// Do not collect terminal work, clone a handle/frame or advance a queue on
    /// this read path. Actual cancellation still uses its ordinary handler.
    pub(crate) fn check_retained_or_historical_job(&self, id: JobId) -> Result<(), JobError> {
        self.check_owner(id)?;
        if self.live.iter().any(|job| job.handle.id() == id)
            || self.completed.iter().any(|result| result.handle.id() == id)
            || self.history.iter().any(|status| status.job == id)
        {
            Ok(())
        } else {
            Err(JobError::new(ErrorCode::UnknownJob, "render job is no longer retained"))
        }
    }

    /// Snapshot existing status without collecting or evicting registry owners.
    /// Native queue polling uses this path; ordinary status/result APIs retain
    /// their existing explicit collection semantics.
    pub(crate) fn peek_status(&self, id: JobId) -> Result<JobStatus, JobError> {
        self.check_owner(id)?;
        if let Some(handle) =
            self.live.iter().map(|job| &job.handle).chain(self.completed.iter().map(|result| &result.handle)).find(|handle| handle.id() == id)
        {
            return handle.status();
        }
        self.history
            .iter()
            .find(|status| status.job == id)
            .cloned()
            .ok_or_else(|| JobError::new(ErrorCode::UnknownJob, "render job is no longer retained"))
    }

    pub(crate) fn status(&mut self, id: JobId) -> Result<JobStatus, JobError> {
        self.check_owner(id)?;
        self.collect()?;
        if let Ok(handle) = self.handle(id) {
            return handle.status();
        }
        self.history.iter().find(|s| s.job == id).cloned().ok_or_else(|| JobError::new(ErrorCode::UnknownJob, "render job is no longer retained"))
    }

    pub(crate) fn pin(&mut self, id: Option<JobId>) -> Result<Option<solidcraft_render::job::FrameInfo>, JobError> {
        self.collect()?;
        let Some(id) = id else {
            self.selected = None;
            self.comparison = None;
            return Ok(None);
        };
        let handle = self.handle(id)?;
        let frame = handle.frame()?.ok_or_else(|| JobError::new(ErrorCode::NotCompleted, "only a completed result can be pinned"))?;
        let info = frame.info.clone();
        self.comparison = None;
        self.selected = Some(id);
        Ok(Some(info))
    }

    pub(crate) fn result_index(&mut self, index: usize, discard: bool) -> Result<(), JobError> {
        self.collect()?;
        let id = self.completed.get(index).ok_or_else(|| JobError::new(ErrorCode::UnknownJob, "retained result index is out of bounds"))?.handle.id();
        if discard {
            self.completed.remove(index);
            if self.selected == Some(id) {
                self.selected = self.completed.back().map(|result| result.handle.id());
            }
        } else {
            self.comparison = None;
            self.selected = Some(id);
        }
        Ok(())
    }

    pub(crate) fn frame(&mut self, id: JobId) -> Result<Arc<RenderFrame>, JobError> {
        self.collect()?;
        self.handle(id)?.frame()?.ok_or_else(|| JobError::new(ErrorCode::NotCompleted, "render has no completed image"))
    }
}

impl crate::Session {
    pub fn render_comparison(&self) -> Option<[Option<JobId>; 2]> {
        self.render_jobs.valid_comparison()
    }

    pub fn render_queue_state(&self) -> RenderQueueState {
        let active = self.render_jobs.batch_job.and_then(|id| self.render_jobs.live.iter().find(|job| job.handle.id() == id));
        RenderQueueState {
            running: self.render_jobs.live.iter().filter(|job| job.handle.status().is_ok_and(|status| !status.progress.phase.terminal())).count(),
            waiting: self
                .render_jobs
                .native_batch
                .as_ref()
                .map_or(self.render_jobs.waiting.len(), |n| n.total.saturating_sub(n.next + usize::from(n.claimed))),
            preparing: self.render_jobs.native_batch.as_ref().is_some_and(|n| n.claimed),
            job: self.render_jobs.batch_job,
            shot: active.and_then(|job| job.shot.clone()),
            error: self.render_jobs.batch_error.clone(),
        }
    }

    pub fn render_batch_ready(&self) -> std::result::Result<(), String> {
        self.render_batch_ready_with_claim(false)
    }

    /// Recheck ordinary Batch enablement without treating this exact first
    /// preparation's own fixed claim as a competing queue.
    pub(crate) fn render_batch_ready_for_first(&self, ticket: crate::render_preparation::batch::Ticket) -> std::result::Result<(), String> {
        if !ticket.matches(self) || !self.render_jobs.native_batch.as_ref().is_some_and(|n| n.ticket == ticket && n.claimed && n.next == 0) {
            return Err("native first shot claim is no longer current".into());
        }
        self.render_batch_ready_with_claim(true)
    }

    fn render_batch_ready_with_claim(&self, own_first_claim: bool) -> std::result::Result<(), String> {
        if self.doc.render.setups.is_empty() {
            return Err("no saved shots to render".into());
        }
        if self.doc.render.setups.len() > 32 {
            return Err("render all shots is limited to 32 shots".into());
        }
        if (!own_first_claim && self.render_jobs.native_batch.is_some())
            || self.render_jobs.batch_job.is_some()
            || self.render_queue_state().running != 0
        {
            return Err("a render or shot queue is already active".into());
        }
        Ok(())
    }

    pub(crate) fn start_render_batch(&mut self, shots: VecDeque<BatchShot>) -> crate::Result<usize> {
        self.render_batch_ready().map_err(crate::EngineError::Other)?;
        if shots.is_empty() || shots.len() > 32 {
            return Err(crate::EngineError::Other("shot queue must contain 1..32 shots".into()));
        }
        let queued = shots.len();
        self.render_jobs.waiting = shots;
        self.render_jobs.batch_error = None;
        self.start_next_batch_shot()?;
        Ok(queued)
    }

    fn start_next_batch_shot(&mut self) -> crate::Result<()> {
        let Some(shot) = self.render_jobs.waiting.pop_front() else { return Ok(()) };
        let result: crate::Result<()> = (|| {
            let mut params = (*shot.request).clone();
            let settings = params
                .get_mut("settings")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or_else(|| crate::EngineError::Other("queued render settings are missing".into()))?;
            settings.insert("camera".into(), serde_json::json!(shot.camera));
            let reply = self.execute("render.start", &params)?;
            let job: JobId = serde_json::from_value(reply.get("job").cloned().unwrap_or_default())
                .map_err(|error| crate::EngineError::Other(format!("invalid queued render identity: {error}")))?;
            self.label_render_job(job, &shot.name)?;
            self.render_jobs.batch_job = Some(job);
            Ok(())
        })();
        if let Err(error) = &result {
            self.render_jobs.waiting.clear();
            self.render_jobs.batch_error = Some(error.to_string());
        }
        result
    }

    /// Non-blocking queue scheduling. The existing host still owns all render work.
    pub fn advance_render_batch(&mut self) -> crate::Result<()> {
        // Native foreground/background status polling never captures a shot.
        if self.render_jobs.native_batch.is_some() {
            if self.render_jobs.native_batch.as_ref().is_some_and(|n| !n.ticket.matches(self)) {
                if let Some(native) = self.render_jobs.native_batch.take() {
                    native.cancel.cancel();
                }
                self.render_jobs.batch_job = None;
                self.render_jobs.batch_error = Some("native shot queue belongs to a previous document owner".into());
                return Ok(());
            }
            if self.render_jobs.native_batch.as_ref().is_some_and(|n| n.cancel.is_cancelled()) {
                self.render_jobs.native_batch = None;
                self.render_jobs.batch_job = None;
                self.render_jobs.batch_error = Some("native shot queue was cancelled".into());
                return Ok(());
            }
            if let Some(job) = self.render_jobs.batch_job {
                let status = self.render_jobs.peek_status(job).map_err(job_error)?;
                if status.progress.phase.terminal() {
                    let failed = status.progress.phase != Phase::Completed;
                    let finished = self.render_jobs.native_batch.as_ref().is_some_and(|n| n.next == n.total && !n.claimed);
                    if failed || finished {
                        if let Some(native) = self.render_jobs.native_batch.take()
                            && failed
                        {
                            native.cancel.cancel();
                        }
                        self.render_jobs.batch_job = None;
                        if failed {
                            self.render_jobs.batch_error = status.error.map(|e| e.to_string());
                        }
                    }
                }
            }
            return Ok(());
        }
        let Some(job) = self.render_jobs.batch_job else { return Ok(()) };
        let status = self.render_jobs.status(job).map_err(job_error)?;
        if !status.progress.phase.terminal() {
            return Ok(());
        }
        self.render_jobs.batch_job = None;
        if status.progress.phase != Phase::Completed {
            self.render_jobs.waiting.clear();
            self.render_jobs.batch_error = status.error.map(|error| error.to_string());
            return Ok(());
        }
        self.start_next_batch_shot()
    }

    pub(crate) fn cancel_render_batch(&mut self, job: JobId) {
        if self.render_jobs.batch_job == Some(job) {
            if let Some(native) = self.render_jobs.native_batch.take() {
                native.cancel.cancel();
            }
            self.render_jobs.waiting.clear();
            self.render_jobs.batch_job = None;
        }
    }

    pub(crate) fn cancel_native_preparation(&mut self) -> bool {
        if self.render_jobs.batch_job.is_none() && self.render_jobs.native_batch.as_ref().is_some_and(|n| n.claimed) {
            if let Some(native) = self.render_jobs.native_batch.take() {
                native.cancel.cancel();
            }
            true
        } else {
            false
        }
    }

    /// Capture the UI draft's label alongside an unnamed active request, never in the design.
    pub fn label_render_job(&mut self, id: JobId, shot: &str) -> crate::Result<()> {
        solidcraft_doc::render_settings::RenderSettings::validate_name(shot).map_err(|error| crate::EngineError::Other(error.to_string()))?;
        let job = self
            .render_jobs
            .live
            .iter_mut()
            .find(|job| job.handle.id() == id)
            .ok_or_else(|| job_error(JobError::new(ErrorCode::UnknownJob, "render job is no longer active")))?;
        if job.shot.is_none() {
            job.shot = Some(shot.to_owned());
        }
        Ok(())
    }

    /// Oldest first, capped at eight. Staleness follows the current design revision.
    pub fn render_retained_results(&mut self) -> crate::Result<Vec<RetainedResult>> {
        self.render_jobs.collect().map_err(job_error)?;
        Ok(self
            .render_jobs
            .completed
            .iter()
            .map(|result| {
                let mut info = result.info.clone();
                info.stale = info.frame.scene_generation != self.revision;
                info
            })
            .collect())
    }

    pub fn render_selected_result(&self) -> Option<JobId> {
        self.render_jobs.selected
    }

    /// A host is installed by the application, never by document data or a command.
    pub fn set_render_job_host(&mut self, host: Option<CpuHost>) -> crate::Result<()> {
        if self.render_jobs.host.as_ref().zip(host.as_ref()).is_some_and(|(a, b)| a.same_host(b)) {
            return Ok(());
        }
        let descriptor = host.as_ref().map(CpuHost::descriptor).transpose().map_err(job_error)?;
        if descriptor.as_ref().is_some_and(|d| d.id.as_str() == "cpu-raytrace") {
            return Err(crate::EngineError::Other("cpu-raytrace is reserved for the native CPU raytrace integrator".into()));
        }
        let previous_raytrace = cpu_raytrace_descriptor(self.render_jobs.host.as_ref());
        let mut backend = self.render_backend();
        backend.executors.retain(|e| {
            self.render_jobs.descriptor.as_ref().is_none_or(|old| old.id != e.id) && previous_raytrace.as_ref().is_none_or(|old| old.id != e.id)
        });
        if let Some(d) = &descriptor {
            backend.executors.push(d.clone());
        }
        if let Some(raytrace) = cpu_raytrace_descriptor(host.as_ref()) {
            backend.executors.push(raytrace);
        }
        solidcraft_render::resolve_request(
            &[backend],
            &solidcraft_render::RenderRequest {
                backend: None,
                mode: solidcraft_render::RenderMode::Preview,
                operation: solidcraft_render::RenderOperation::Image,
                width: 1,
                height: 1,
                format: Some(solidcraft_render::RenderFormat::Png),
                features: Vec::new(),
                policy: Default::default(),
            },
        )
        .map_err(|e| crate::EngineError::Other(e.to_string()))?;
        let mut jobs = Jobs::default();
        jobs.additional = self.render_jobs.additional.clone();
        jobs.host = host;
        jobs.descriptor = descriptor;
        self.render_jobs = jobs;
        Ok(())
    }

    /// Add an executor without replacing CPU jobs or their ownership. Applications share
    /// this host across document sessions; descriptor IDs must be unique in the registry.
    pub fn register_render_job_host(&mut self, host: solidcraft_render::job::ImageHost) -> crate::Result<()> {
        let descriptor = host.descriptor().map_err(job_error)?;
        if descriptor.id.as_str() == "cpu-raytrace" {
            return Err(crate::EngineError::Other("cpu-raytrace is reserved for the native CPU raytrace integrator".into()));
        }
        if self.render_jobs.additional.iter().any(|(old, _)| old.same_host(&host)) {
            return Ok(());
        }
        if descriptor.profiles.iter().any(|p| p.operation != solidcraft_render::RenderOperation::Image) {
            return Err(crate::EngineError::Other("image host must only register image profiles".into()));
        }
        let mut registry = self.render_backend();
        registry.executors.push(descriptor.clone());
        solidcraft_render::resolve_request(
            &[registry],
            &solidcraft_render::RenderRequest {
                backend: None,
                mode: solidcraft_render::RenderMode::Preview,
                operation: solidcraft_render::RenderOperation::Image,
                width: 1,
                height: 1,
                format: Some(solidcraft_render::RenderFormat::Png),
                features: Vec::new(),
                policy: Default::default(),
            },
        )
        .map_err(|e| crate::EngineError::Other(e.to_string()))?;
        self.render_jobs.additional.push((host, descriptor));
        Ok(())
    }

    pub fn render_job_host(&self) -> Option<&CpuHost> {
        self.render_jobs.host.as_ref()
    }

    /// Explicit native CLI waiting is separate from non-blocking status commands.
    pub fn render_job_handle(&self, id: JobId) -> crate::Result<JobHandle> {
        self.render_jobs.handle(id).map_err(job_error)
    }
}

pub(crate) fn job_error(error: JobError) -> crate::EngineError {
    crate::EngineError::Other(error.to_string())
}

#[cfg(test)]
mod displayed_result_tests {
    use super::*;
    use serde_json::json;
    fn complete(s: &mut crate::Session, color: [u8; 3]) -> JobId {
        let reply = s.execute("render.start", &json!({"settings":{"width":8,"height":8,"background":color}})).unwrap();
        let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
        let host = s.render_job_host().unwrap().clone();
        for _ in 0..10000 {
            host.pump(16384).unwrap();
            if s.render_job_handle(id).unwrap().status().unwrap().progress.phase.terminal() {
                return id;
            }
        }
        panic!("bounded result fixture did not finish")
    }
    #[test]
    fn explicit_display_pin_selects_exact_bytes_within_bounded_retention() {
        let mut s = crate::Session::default();
        let host = CpuHost::cooperative();
        s.set_render_job_host(Some(host.clone())).unwrap();
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let old = complete(&mut s, [10, 20, 30]);
        let bytes = s.render_job_handle(old).unwrap().frame().unwrap().unwrap().png().to_vec();
        s.execute("render.result.pin", &json!({"job":old})).unwrap();
        for color in [[40, 50, 60], [70, 80, 90], [100, 110, 120], [130, 140, 150], [160, 170, 180]] {
            let new = complete(&mut s, color);
            s.execute("render.status", &json!({"job":new})).unwrap();
            assert_eq!(s.render_job_handle(old).unwrap().frame().unwrap().unwrap().png(), bytes);
        }
        let retained = host.admitted_bytes();
        let foreign = JobId { session: old.session + 1, ..old };
        assert!(s.execute("render.result.pin", &json!({"job":foreign})).is_err());
        assert_eq!(host.admitted_bytes(), retained);
        assert_eq!(s.execute("render.result.pin", &json!({})).unwrap()["frame"]["job"], json!(s.render_selected_result()));
        s.execute("render.result.pin", &json!({"job":null})).unwrap();
        assert!(s.render_selected_result().is_none());
        assert!(s.render_job_handle(old).is_ok(), "clearing selection does not discard retained results");
        assert_eq!(host.admitted_bytes(), retained);
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
        let reply = s.execute("render.start", &json!({"settings":{"width":8,"height":8}})).unwrap();
        assert!(s.execute("render.result.pin", &json!({"job":reply["job"]})).is_err(), "a pending result is never exportable");
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn first_native_plan(s: &mut crate::Session) -> (crate::render_preparation::batch::Plan, JobId) {
        use crate::render_preparation::batch;
        use solidcraft_render::job::CancelToken;
        let mut params = Some(json!({}));
        let claim = batch::claim(s, &mut params, &mut None, CancelToken::default()).unwrap();
        let (ready, receiver) = std::sync::mpsc::sync_channel(1);
        let (returning, returned) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            batch::with_scope(claim, |claim| {
                ready.send(batch::prepare(claim)).unwrap();
                drop(returned.recv().unwrap());
            });
        });
        let mut prepared = receiver.recv().unwrap();
        assert_eq!(batch::finish_retained(s, &mut prepared).unwrap(), json!({"queued":2}));
        let plan = batch::take_plan(&mut prepared).unwrap().unwrap();
        returning.send(prepared).unwrap();
        thread.join().unwrap();
        (plan, s.render_queue_state().job.unwrap())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_next_discard_uses_cached_status_but_hostile_missing_status_fails_only_its_exact_ticket() {
        use crate::render_preparation::batch;
        use solidcraft_render::job::CancelToken;
        for hostile_missing_status in [false, true] {
            let mut s = crate::Session::default();
            let host = CpuHost::cooperative();
            s.set_render_job_host(Some(host.clone())).unwrap();
            let old = complete(&mut s, [11, 23, 37]);
            let old_handle = s.render_job_handle(old).unwrap();
            let old_frame = old_handle.frame().unwrap().unwrap();
            let old_png = old_frame.png().to_vec();
            for name in ["A", "B"] {
                s.execute("render.settings.set", &json!({"name":name,"set":{"width":2,"height":1,"job":{"method":"raster"}}})).unwrap();
            }
            let (plan, first) = first_native_plan(&mut s);
            for _ in 0..10_000 {
                if s.render_job_handle(first).unwrap().status().unwrap().progress.phase.terminal() {
                    break;
                }
                host.pump(4096).unwrap();
            }
            assert_eq!(s.render_job_handle(first).unwrap().status().unwrap().progress.phase, Phase::Completed);
            let retained = s.render_retained_results().unwrap();
            let index = retained.iter().position(|r| r.frame.job == first).unwrap();
            s.execute("render.result.discard", &json!({"index":index})).unwrap();
            assert!(s.render_job_handle(first).is_err());
            assert_eq!(
                s.render_jobs.status(first).unwrap().progress.phase,
                Phase::Completed,
                "ordinary Discard preserves the recent status cache and is not an UnknownJob reproducer"
            );
            if hostile_missing_status {
                // Defensive hostile registry-state injection, not an ordinary
                // UI Discard claim: public regrade/replace cannot rotate this
                // status while preserving the same native ticket.
                s.render_jobs.history.retain(|status| status.job != first);
            }
            let bytes = plan.retained_bytes();
            let held = host.admitted_bytes();
            let mut slot = Some(plan);
            let result = batch::claim_next(&mut s, &mut slot, CancelToken::default());
            if hostile_missing_status {
                let error = match result {
                    Err(error) => error,
                    Ok(_) => panic!("missing current status must refuse"),
                };
                let message = error.to_string();
                assert!(message.contains("render job is no longer retained"));
                assert_eq!(s.render_queue_state().error.as_deref(), Some(message.as_str()));
                assert_eq!((s.render_queue_state().waiting, s.render_queue_state().preparing), (0, false));
                assert!(s.render_queue_state().job.is_none());
                assert!(slot.is_some());
                assert_eq!(host.admitted_bytes(), held, "metadata failure cannot release the retained Plan backing");
                assert_eq!(old_frame.png(), old_png);
                assert_eq!(s.render_jobs.status(old).unwrap().progress.phase, Phase::Completed);
                let token = CancelToken::default();
                let replacement = batch::claim(&mut s, &mut Some(json!({})), &mut None, token.clone()).unwrap();
                let current = json!(s.render_queue_state());
                assert!(batch::claim_next(&mut s, &mut slot, CancelToken::default()).is_err());
                assert_eq!(json!(s.render_queue_state()), current, "stale Plan refusal cannot fail a replacement epoch");
                assert!(!token.is_cancelled());
                let before = host.admitted_bytes();
                let retired = slot.take().unwrap();
                std::thread::spawn(move || batch::retire_plan(retired)).join().unwrap();
                assert_eq!(host.admitted_bytes(), before - bytes, "only actual worker retirement releases Plan credit");
                batch::cancel(&mut s).unwrap();
                std::thread::spawn(move || batch::with_scope(replacement, drop)).join().unwrap();
            } else {
                let claim = result.unwrap().unwrap();
                assert!(slot.is_none());
                assert_eq!(s.render_queue_state().waiting, 0);
                assert!(s.render_queue_state().preparing);
                assert_eq!(host.admitted_bytes(), held);
                batch::cancel(&mut s).unwrap();
                std::thread::spawn(move || batch::with_scope(claim, drop)).join().unwrap();
                assert_eq!(host.admitted_bytes(), held - bytes);
            }
            assert_eq!(old_frame.png(), old_png);
            assert_eq!(old_handle.status().unwrap().progress.phase, Phase::Completed);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn immutable_native_cancel_identity_checks_live_completed_and_discarded_history_without_collecting_or_advancing() {
        use crate::render_preparation::batch;
        let mut s = crate::Session::default();
        let host = CpuHost::cooperative();
        s.set_render_job_host(Some(host.clone())).unwrap();
        for name in ["A", "B"] {
            s.execute("render.settings.set", &json!({"name":name,"set":{"width":2,"height":1,"job":{"method":"raster"}}})).unwrap();
        }
        let (plan, job) = first_native_plan(&mut s);
        let document = s.doc.clone();
        let model = s.model.state();
        let handle = s.render_job_handle(job).unwrap();
        let check = |s: &crate::Session| {
            let identities = || {
                (
                    s.render_jobs.live.iter().map(|job| job.handle.id()).collect::<Vec<_>>(),
                    s.render_jobs.completed.iter().map(|result| result.handle.id()).collect::<Vec<_>>(),
                    s.render_jobs.history.iter().map(|status| status.job).collect::<Vec<_>>(),
                    s.render_jobs.serial,
                    s.render_jobs.generation,
                    json!(s.render_queue_state()),
                )
            };
            let before = identities();
            let bytes = host.admitted_bytes();
            let owners = (Arc::strong_count(&document), Arc::strong_count(&model));
            let history = (s.revision, s.undo.len(), s.redo.len());
            assert_eq!(batch::check_cancel(s, &json!({"job":job})).unwrap(), Some(job));
            assert_eq!(batch::check_cancel(s, &json!({})).unwrap(), None);
            assert_eq!(identities(), before, "pure cancel validation cannot collect or advance a native shot front");
            assert_eq!(host.admitted_bytes(), bytes, "validation neither reserves nor drops renderer/plan backing");
            assert_eq!((Arc::strong_count(&document), Arc::strong_count(&model)), owners, "no source clone or retirement");
            assert!(Arc::ptr_eq(&s.doc, &document));
            assert!(Arc::ptr_eq(&s.model.state(), &model));
            assert_eq!((s.revision, s.undo.len(), s.redo.len()), history);
        };
        assert_eq!(handle.status().unwrap().progress.phase, Phase::Queued);
        check(&s);
        for _ in 0..10_000 {
            if !host.pump(4096).unwrap() {
                break;
            }
        }
        assert_eq!(handle.status().unwrap().progress.phase, Phase::Completed);
        assert_eq!((s.render_jobs.live.len(), s.render_jobs.completed.len(), s.render_jobs.history.len()), (1, 0, 0));
        check(&s);
        assert_eq!(
            (s.render_jobs.live.len(), s.render_jobs.completed.len(), s.render_jobs.history.len()),
            (1, 0, 0),
            "a completed-but-uncollected handle remains in live metadata"
        );
        s.render_jobs.collect().unwrap();
        assert_eq!((s.render_jobs.live.len(), s.render_jobs.completed.len(), s.render_jobs.history.len()), (0, 1, 1));
        check(&s);
        let frame = handle.frame().unwrap().unwrap();
        let png = frame.png().to_vec();
        s.execute("render.result.discard", &json!({"index":0})).unwrap();
        assert_eq!((s.render_jobs.live.len(), s.render_jobs.completed.len(), s.render_jobs.history.len()), (0, 0, 1));
        assert!(s.render_job_handle(job).is_err());
        check(&s);
        assert_eq!(frame.png(), png, "checking an actual discarded result uses history, not a replacement image");
        let unknown = JobId { serial: u64::MAX, generation: u64::MAX, ..job };
        let foreign = JobId { session: job.session + 1, ..job };
        for (id, code) in [(unknown, ErrorCode::UnknownJob), (foreign, ErrorCode::WrongOwner)] {
            let before = json!(s.render_queue_state());
            let bytes = host.admitted_bytes();
            assert_eq!(s.render_jobs.check_retained_or_historical_job(id).unwrap_err().code, code);
            assert!(batch::check_cancel(&s, &json!({"job":id})).is_err());
            assert_eq!(json!(s.render_queue_state()), before);
            assert_eq!(host.admitted_bytes(), bytes);
        }
        assert_eq!((s.render_queue_state().waiting, s.render_queue_state().preparing), (1, false));
        let held = host.admitted_bytes();
        let plan_bytes = plan.retained_bytes();
        assert!(batch::cancel(&mut s).unwrap(), "terminal history is sufficient to cancel the remaining native queue");
        assert_eq!((s.render_queue_state().waiting, s.render_queue_state().preparing, s.render_queue_state().job), (0, false, None));
        assert_eq!((s.render_jobs.live.len(), s.render_jobs.completed.len(), s.render_jobs.history.len()), (0, 0, 1));
        assert_eq!(host.admitted_bytes(), held, "metadata cancellation retains the Plan lease until its worker retires it");
        assert_eq!(frame.png(), png);
        assert!(Arc::ptr_eq(&s.doc, &document) && Arc::ptr_eq(&s.model.state(), &model));
        std::thread::spawn(move || batch::retire_plan(plan)).join().unwrap();
        assert_eq!(host.admitted_bytes(), held - plan_bytes);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_queue_status_and_next_claim_keep_eight_results_and_terminal_live_backing_until_explicit_collection() {
        use crate::render_preparation::batch;
        use solidcraft_render::job::CancelToken;
        for cancelled in [false, true] {
            let mut s = crate::Session::default();
            let host = CpuHost::cooperative();
            s.set_render_job_host(Some(host.clone())).unwrap();
            let mut old_ids = Vec::new();
            let mut oldest_frame = None;
            for index in 0..8 {
                let id = complete(&mut s, [index, 31, 63]);
                s.render_retained_results().unwrap();
                old_ids.push(id);
                if index == 0 {
                    oldest_frame = Some(Arc::downgrade(&s.render_job_handle(id).unwrap().frame().unwrap().unwrap()));
                }
            }
            let oldest_frame = oldest_frame.unwrap();
            assert_eq!(s.render_jobs.completed.len(), 8);
            for name in ["A", "B"] {
                s.execute("render.settings.set", &json!({"name":name,"set":{"width":2,"height":1,"job":{"method":"raster"}}})).unwrap();
            }
            let (plan, job) = first_native_plan(&mut s);
            let handle = s.render_job_handle(job).unwrap();
            if cancelled {
                assert_eq!(handle.cancel().unwrap().progress.phase, Phase::Cancelled);
            } else {
                for _ in 0..10_000 {
                    if !host.pump(4096).unwrap() {
                        break;
                    }
                }
                assert_eq!(handle.status().unwrap().progress.phase, Phase::Completed);
            }
            let actual = handle.status().unwrap();
            let peek = s.render_jobs.peek_status(job).unwrap();
            assert_eq!((peek.job, peek.progress.phase), (job, actual.progress.phase));
            assert_eq!(peek.error.as_ref().map(|e| (e.code, e.message.as_str())), actual.error.as_ref().map(|e| (e.code, e.message.as_str())));
            let foreign = JobId { session: job.session + 1, ..job };
            let unknown = JobId { serial: u64::MAX, generation: u64::MAX, ..job };
            assert_eq!(s.render_jobs.peek_status(foreign).unwrap_err().code, ErrorCode::WrongOwner);
            assert_eq!(s.render_jobs.peek_status(unknown).unwrap_err().code, ErrorCode::UnknownJob);
            let held = host.admitted_bytes();
            let membership = |s: &crate::Session| {
                (
                    s.render_jobs.live.iter().map(|j| j.handle.id()).collect::<Vec<_>>(),
                    s.render_jobs.completed.iter().map(|j| j.handle.id()).collect::<Vec<_>>(),
                    s.render_jobs.history.iter().map(|j| j.job).collect::<Vec<_>>(),
                )
            };
            let before = membership(&s);
            assert_eq!(before.0, vec![job]);
            assert_eq!(before.1, old_ids);
            assert!(oldest_frame.upgrade().is_some());
            s.advance_render_batch().unwrap();
            assert_eq!(membership(&s), before, "native status polling cannot collect a terminal live job or evict result0");
            assert_eq!(host.admitted_bytes(), held);
            assert!(oldest_frame.upgrade().is_some());
            let mut slot = Some(plan);
            let claim = batch::claim_next(&mut s, &mut slot, CancelToken::default());
            assert_eq!(membership(&s), before, "metadata-only next-front claim cannot drive ordinary result retention");
            assert_eq!(host.admitted_bytes(), held);
            assert!(oldest_frame.upgrade().is_some());
            if cancelled {
                assert!(claim.is_err());
                assert!(slot.is_some());
                assert_eq!((s.render_queue_state().waiting, s.render_queue_state().preparing), (0, false));
                assert!(s.render_queue_state().job.is_none());
                assert_eq!(s.render_queue_state().error.as_deref(), Some("native shot queue was cancelled"));
                let bytes = slot.as_ref().unwrap().retained_bytes();
                let retired = slot.take().unwrap();
                std::thread::spawn(move || batch::retire_plan(retired)).join().unwrap();
                assert_eq!(host.admitted_bytes(), held - bytes);
                let rows = s.render_retained_results().unwrap();
                assert_eq!(rows.iter().map(|r| r.frame.job).collect::<Vec<_>>(), old_ids);
                assert!(s.render_jobs.live.is_empty());
                assert!(oldest_frame.upgrade().is_some());
            } else {
                let claim = claim.unwrap().unwrap();
                assert!(slot.is_none());
                assert_eq!((s.render_queue_state().waiting, s.render_queue_state().preparing), (0, true));
                assert_eq!(s.render_queue_state().job, Some(job), "keep the last real job while preparing the next camera");
                // Ordinary explicit result collection still applies its existing8-row policy.
                let rows = s.render_retained_results().unwrap();
                let mut expected = old_ids[1..].to_vec();
                expected.push(job);
                assert_eq!(rows.iter().map(|r| r.frame.job).collect::<Vec<_>>(), expected);
                assert!(s.render_jobs.live.is_empty());
                assert!(oldest_frame.upgrade().is_none(), "only explicit ordinary collection evicts the oldest completed frame");
                assert!(host.admitted_bytes() < held);
                batch::cancel(&mut s).unwrap();
                std::thread::spawn(move || batch::with_scope(claim, drop)).join().unwrap();
            }
        }
    }
}
