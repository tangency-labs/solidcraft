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
}

pub(crate) struct Jobs {
    owner: u64,
    serial: u64,
    generation: u64,
    pub(crate) host: Option<CpuHost>,
    pub(crate) descriptor: Option<ExecutorDescriptor>,
    pub(crate) additional: Vec<(CpuHost, ExecutorDescriptor)>,
    live: Vec<JobHandle>,
    history: VecDeque<JobStatus>,
    last_good: Option<JobHandle>,
    /// One explicitly displayed completed result, independent of a newer completion.
    /// The existing frame Arc keeps its original host reservation accounted for.
    pinned: Option<JobHandle>,
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
            last_good: None,
            pinned: None,
        }
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        for job in &self.live {
            let _ = job.cancel();
        }
    }
}

impl Jobs {
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
            let status = self.live.get(i).ok_or_else(|| JobError::new(ErrorCode::Internal, "missing live job"))?.status()?;
            if !status.progress.phase.terminal() {
                i += 1;
                continue;
            }
            let handle = self.live.remove(i);
            if status.progress.phase == Phase::Completed && self.last_good.as_ref().is_none_or(|old| old.id().serial < handle.id().serial) {
                self.last_good = Some(handle);
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
        let Execution { executor, options, environment } = execution;
        self.collect()?;
        let host = self.host_for(&executor).ok_or_else(|| JobError::new(ErrorCode::HostUnavailable, "resolved image host is not installed"))?.clone();
        if replace {
            for job in self.live.drain(..) {
                let mut status = job.cancel()?;
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
        self.live.push(handle);
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
        self.live.push(handle);
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
            .chain(self.last_good.iter())
            .chain(self.pinned.iter())
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
        let Some(id) = id else {
            self.pinned = None;
            return Ok(None);
        };
        let handle = self.handle(id)?;
        let frame = handle.frame()?.ok_or_else(|| JobError::new(ErrorCode::NotCompleted, "only a completed result can be pinned"))?;
        let info = frame.info.clone();
        self.pinned = Some(handle);
        Ok(Some(info))
    }

    pub(crate) fn frame(&mut self, id: JobId) -> Result<Arc<RenderFrame>, JobError> {
        self.collect()?;
        self.handle(id)?.frame()?.ok_or_else(|| JobError::new(ErrorCode::NotCompleted, "render has no completed image"))
    }
}

impl crate::Session {
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
    fn explicit_display_pin_preserves_the_old_exact_frame_without_unbounded_history() {
        let mut s = crate::Session::default();
        let host = CpuHost::cooperative();
        s.set_render_job_host(Some(host.clone())).unwrap();
        let before = s.doc.to_json();
        let undo = s.history.len();
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
        assert!(s.execute("render.result.pin", &json!({})).is_err());
        s.execute("render.result.pin", &json!({"job":null})).unwrap();
        assert!(s.render_job_handle(old).is_err());
        assert!(host.admitted_bytes() < retained);
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.history.len(), undo);
        let reply = s.execute("render.start", &json!({"settings":{"width":8,"height":8}})).unwrap();
        assert!(s.execute("render.result.pin", &json!({"job":reply["job"]})).is_err(), "a pending result is never exportable");
    }
}
