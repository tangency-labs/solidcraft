//! Application cloud services are shared; captures and saved baselines belong to each editor.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use solidcraft_cloud::{
    Destination, Settings,
    jobs::{Host, Job, JobStatus, Phase},
};

use crate::{Document, EngineError, Result, Session, Visibility};

mod management;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Binding {
    pub destination: Destination,
    pub revision: Option<String>,
}

pub(crate) struct Capture {
    pub doc: Arc<Document>,
    pub visibility: Visibility,
    pub binding: Binding,
}

/// One runtime per application; every editor and background operation shares it.
#[derive(Default)]
pub(crate) struct Runtime {
    pub settings: Settings,
    pub host: Option<Arc<dyn Host>>,
    pub jobs: BTreeMap<String, Job>,
    pub capture_jobs: BTreeSet<String>,
    pub configuration: Option<String>,
}

#[derive(Default)]
pub(crate) struct State {
    runtime: Arc<Mutex<Runtime>>,
    pub captures: BTreeMap<String, Capture>,
    pub binding: Option<Binding>,
    pub ssh_key_input: Option<solidcraft_cloud::jobs::SshKeyInput>,
    pub repository_check_input: Option<solidcraft_cloud::jobs::RepositoryCheckInput>,
}

impl State {
    pub fn runtime(&self) -> MutexGuard<'_, Runtime> {
        // The runtime holds only handles and validated settings. Recovering the lock
        // leaves durable data in the platform store and lets failed jobs report errors.
        self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn clear_captures(&mut self) {
        {
            let mut runtime = self.runtime();
            for id in self.captures.keys() {
                runtime.capture_jobs.remove(id);
            }
        }
        self.captures.clear();
    }

    pub fn scratch(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            binding: self.binding.clone(),
            captures: BTreeMap::new(),
            ssh_key_input: None,
            repository_check_input: None,
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.clear_captures();
    }
}

pub(crate) fn error(error: solidcraft_cloud::Error) -> EngineError {
    EngineError::Other(error.to_string())
}

impl Session {
    /// One-shot setup credential, outside recorded/serializable command parameters.
    pub fn set_cloud_repository_check_token(&mut self, token: Option<String>) -> Result<()> {
        self.cloud.repository_check_input = None;
        self.cloud.repository_check_input = Some(solidcraft_cloud::jobs::RepositoryCheckInput::new(token).map_err(error)?);
        Ok(())
    }

    /// Supply a passphrase directly, outside the command recorder. The next
    /// cloud.ssh.generate consumes it, including when submission fails.
    pub fn set_cloud_ssh_key_input(&mut self, input: solidcraft_cloud::jobs::SshKeyInput) -> Result<()> {
        self.cloud.ssh_key_input = None;
        input.validate().map_err(error)?;
        self.cloud.ssh_key_input = Some(input);
        Ok(())
    }

    pub fn clear_cloud_ssh_key_input(&mut self) {
        self.cloud.ssh_key_input = None;
    }

    pub fn set_cloud_host(&mut self, host: Option<Arc<dyn Host>>) {
        self.cloud.runtime().host = host;
    }

    pub fn cloud_settings(&self) -> Settings {
        self.cloud.runtime().settings.clone()
    }

    /// Host preferences are shared by tabs, while bindings and captured saves stay per design.
    pub fn inherit_cloud_services(&mut self, source: &Session) {
        self.cloud.runtime = source.cloud.runtime.clone();
    }

    pub fn cloud_configuring(&self) -> bool {
        self.cloud.runtime().configuration.is_some()
    }

    pub fn cloud_save_pending(&self) -> bool {
        !self.cloud.captures.is_empty()
    }
    pub fn cloud_binding(&self) -> Option<&Binding> {
        self.cloud.binding.as_ref()
    }

    pub fn cloud_operations(&self) -> Result<Vec<JobStatus>> {
        self.cloud.runtime().jobs.values().map(|job| job.status().map_err(error)).collect()
    }

    /// Complete a download through the registered command in a separate editor. The UI can
    /// adopt it as a new tab without replacing edits made while the download was running.
    pub fn cloud_open_session(&self, operation: &str) -> Result<Session> {
        let mut opened = self.cloud_open_target(operation)?;
        opened.execute("cloud.open.finish", &serde_json::json!({"operation":operation}))?;
        Ok(opened)
    }

    /// Prepare an isolated editor without decoding or evaluating on the caller's thread.
    pub fn cloud_open_target(&self, operation: &str) -> Result<Session> {
        if !self.cloud.runtime().jobs.contains_key(operation) {
            return Err(EngineError::Other("unknown cloud opening operation".into()));
        }
        let mut opened = Session::default();
        opened.inherit_cloud_services(self);
        Ok(opened)
    }

    /// A captured save may become the saved baseline after later edits. Adopting the captured
    /// Arc instead of calling mark_saved keeps those newer edits dirty.
    pub fn poll_cloud(&mut self) -> Result<()> {
        {
            let mut runtime = self.cloud.runtime();
            if let Some(id) = runtime.configuration.clone() {
                let status =
                    runtime.jobs.get(&id).ok_or_else(|| EngineError::Other("cloud settings operation missing".into()))?.status().map_err(error)?;
                match status.phase {
                    Phase::Ready | Phase::Synced => {
                        let settings: Settings =
                            serde_json::from_value(status.result.ok_or_else(|| EngineError::Other("cloud settings missing".into()))?)
                                .map_err(|e| EngineError::Other(e.to_string()))?;
                        settings.validate().map_err(error)?;
                        runtime.settings = settings;
                        runtime.configuration = None;
                    }
                    Phase::Failed | Phase::Cancelled => runtime.configuration = None,
                    _ => {}
                }
            }
        }
        let ids: Vec<String> = self.cloud.captures.keys().cloned().collect();
        for id in ids {
            let status = self
                .cloud
                .runtime()
                .jobs
                .get(&id)
                .ok_or_else(|| EngineError::Other("cloud save operation missing".into()))?
                .status()
                .map_err(error)?;
            if status.saved_locally {
                if status
                    .revision
                    .as_ref()
                    .is_none_or(|revision| revision.len() != 64 || !revision.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
                {
                    return Err(EngineError::Other("cloud save acknowledgement is missing its captured revision".into()));
                }
                let capture = self.cloud.captures.remove(&id).ok_or_else(|| EngineError::Other("cloud save capture missing".into()))?;
                self.cloud.runtime().capture_jobs.remove(&id);
                self.saved = capture.doc;
                self.saved_visibility = capture.visibility;
                let mut binding = capture.binding;
                binding.revision = status.revision;
                self.cloud.binding = Some(binding);
            } else if matches!(status.phase, Phase::Failed | Phase::Cancelled) {
                self.cloud.captures.remove(&id);
                self.cloud.runtime().capture_jobs.remove(&id);
            }
        }
        // A later explicit synchronization can finish older offline Save operations. Apply
        // each confirmed capture even when another file in the same synchronization failed.
        let statuses =
            self.cloud.runtime().jobs.iter().map(|(id, job)| Ok((id.clone(), job.status().map_err(error)?))).collect::<Result<Vec<_>>>()?;
        let mut publications = BTreeMap::new();
        for (_, status) in &statuses {
            for publication in &status.publications {
                publication.destination.validate().map_err(error)?;
                if publication.destination.connection != status.connection {
                    return Err(EngineError::Other("publication belongs to another connection".into()));
                }
            }
        }
        for publication in statuses.iter().flat_map(|(_, status)| &status.publications) {
            publication.destination.validate().map_err(error)?;
            let key = (&publication.destination.connection, &publication.revision);
            if let Some(previous) = publications.insert(key, publication)
                && previous.destination != publication.destination
            {
                return Err(EngineError::Other("conflicting publication destinations".into()));
            }
        }
        for (id, status) in &statuses {
            if status.saved_locally
                && status.phase != Phase::Synced
                && let Some(revision) = &status.revision
                && let Some(publication) = publications.get(&(&status.connection, revision))
                && let Some(job) = self.cloud.runtime().jobs.get(id)
            {
                let result = serde_json::to_value(publication).map_err(|e| EngineError::Other(e.to_string()))?;
                job.update(|status| {
                    status.destination = Some(publication.destination.clone());
                    status.result = Some(result);
                    status.message = None;
                    status.phase = Phase::Synced;
                })
                .map_err(error)?;
            }
        }
        if let Some(binding) = &mut self.cloud.binding
            && let Some(revision) = &binding.revision
            && let Some(publication) = publications.get(&(&binding.destination.connection, revision))
        {
            binding.destination = publication.destination.clone();
        }
        // Publication can preserve an external competing edit by redirecting this capture
        // to a conflict copy. Only its matching current binding follows that result; a later
        // save, a newly opened file, or a native Save As must keep its own destination.
        for (_, status) in statuses {
            if status.phase == Phase::Synced
                && let Some(binding) = &mut self.cloud.binding
                && binding.revision.is_some()
                && binding.revision == status.revision
                && binding.destination.connection == status.connection
                && let Some(destination) = status.destination
            {
                binding.destination = destination;
            }
        }
        self.poll_cloud_management(&[])
    }
}
