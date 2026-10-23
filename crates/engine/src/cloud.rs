//! Cloud state belongs to the editor connection, never to a shared CAD document.
use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_cloud::{
    Destination, Settings,
    jobs::{Host, Job, JobStatus, Phase},
};

use crate::{Document, EngineError, Result, Session, Visibility};

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

#[derive(Default)]
pub(crate) struct State {
    pub settings: Settings,
    pub host: Option<Arc<dyn Host>>,
    pub jobs: BTreeMap<String, Job>,
    pub captures: BTreeMap<String, Capture>,
    pub binding: Option<Binding>,
}

impl State {
    pub fn scratch(&self) -> Self {
        Self { settings: self.settings.clone(), host: self.host.clone(), binding: self.binding.clone(), ..Default::default() }
    }
}

pub(crate) fn error(error: solidcraft_cloud::Error) -> EngineError {
    EngineError::Other(error.to_string())
}

impl Session {
    pub fn set_cloud_host(&mut self, host: Option<Arc<dyn Host>>) {
        self.cloud.host = host;
    }

    pub fn cloud_settings(&self) -> &Settings {
        &self.cloud.settings
    }
    pub fn cloud_binding(&self) -> Option<&Binding> {
        self.cloud.binding.as_ref()
    }

    pub fn cloud_operations(&self) -> Result<Vec<JobStatus>> {
        self.cloud.jobs.values().map(|job| job.status().map_err(error)).collect()
    }

    /// A captured save may become the saved baseline after later edits. Adopting the captured
    /// Arc instead of calling mark_saved keeps those newer edits dirty.
    pub fn poll_cloud(&mut self) -> Result<()> {
        let ids: Vec<String> = self.cloud.captures.keys().cloned().collect();
        for id in ids {
            let status =
                self.cloud.jobs.get(&id).ok_or_else(|| EngineError::Other("cloud save operation missing".into()))?.status().map_err(error)?;
            if status.saved_locally {
                let capture = self.cloud.captures.remove(&id).ok_or_else(|| EngineError::Other("cloud save capture missing".into()))?;
                self.saved = capture.doc;
                self.saved_visibility = capture.visibility;
                self.cloud.binding = Some(capture.binding);
            } else if matches!(status.phase, Phase::Failed | Phase::Cancelled) {
                self.cloud.captures.remove(&id);
            }
        }
        // A later explicit synchronization can finish older offline Save operations. Apply
        // each confirmed capture even when another file in the same synchronization failed.
        let statuses = self.cloud.jobs.iter().map(|(id, job)| Ok((id.clone(), job.status().map_err(error)?))).collect::<Result<Vec<_>>>()?;
        let mut publications = BTreeMap::new();
        for (_, status) in &statuses {
            for publication in &status.publications {
                publication.destination.validate().map_err(error)?;
                if publication.destination.connection != status.connection {
                    return Err(EngineError::Other("publication belongs to another connection".into()));
                }
                let key = (&publication.destination.connection, &publication.revision);
                if let Some(previous) = publications.insert(key, publication)
                    && previous.destination != publication.destination
                {
                    return Err(EngineError::Other("conflicting publication destinations".into()));
                }
            }
        }
        for (id, status) in &statuses {
            if status.saved_locally
                && status.phase != Phase::Synced
                && let Some(revision) = &status.revision
                && let Some(publication) = publications.get(&(&status.connection, revision))
                && let Some(job) = self.cloud.jobs.get(id)
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
                && let Some(destination) = status.destination
            {
                binding.destination = destination;
            }
        }
        Ok(())
    }
}
