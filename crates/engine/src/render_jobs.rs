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

pub(crate) struct Execution {
    pub executor: solidcraft_render::ExecutorId,
    pub options: solidcraft_render::job::JobOptions,
    pub environment: Option<solidcraft_render::lighting::environment_input::EnvironmentInput>,
    pub shot: Option<String>,
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
        }
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        for job in &self.live {
            let _ = job.handle.cancel();
        }
    }
}

impl Jobs {
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
                self.selected = Some(job.handle.id());
                self.completed.push_back(Completed {
                    handle: job.handle,
                    info: RetainedResult { frame: frame.clone(), shot: job.shot, seconds, stale: false },
                });
                while self.completed.len() > 8 {
                    self.completed.pop_front();
                }
            }
            self.history.push_back(status);
            while self.history.len() > 4 {
                self.history.pop_front();
            }
        }
        Ok(())
    }

    pub(crate) fn start_with_environment(
        &mut self,
        revision: u64,
        scene: solidcraft_render::ProductScene,
        request: solidcraft_render::job::ImageRequest,
        replace: bool,
        execution: Execution,
    ) -> Result<JobStatus, JobError> {
        let Execution { executor, options, environment, shot } = execution;
        self.collect()?;
        let host = self.host_for(&executor).ok_or_else(|| JobError::new(ErrorCode::HostUnavailable, "resolved image host is not installed"))?.clone();
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
        let handle = host.submit_with_environment_and_options(
            JobId { session: self.owner, serial, generation },
            revision,
            scene,
            request,
            options,
            environment,
        )?;
        let status = handle.status()?;
        self.serial = serial;
        self.generation = generation;
        self.live.push(LiveJob { handle, shot });
        Ok(status)
    }

    pub(crate) fn regrade(
        &mut self,
        source_id: JobId,
        presentation: solidcraft_render::job::presentation::Presentation,
    ) -> Result<JobStatus, JobError> {
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
            return Ok(None);
        };
        let handle = self.handle(id)?;
        let frame = handle.frame()?.ok_or_else(|| JobError::new(ErrorCode::NotCompleted, "only a completed result can be pinned"))?;
        let info = frame.info.clone();
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
}
