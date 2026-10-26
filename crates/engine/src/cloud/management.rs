//! Apply confirmed management to matching open documents without replacing later edits.
use super::*;
use solidcraft_cloud::management::{Change, Managed};

impl Session {
    pub fn poll_cloud_management(&mut self, confirmed: &[Managed]) -> Result<()> {
        let local = self.cloud.jobs.values().map(|j| j.status().map_err(error)).collect::<Result<Vec<_>>>()?;
        for managed in local.iter().flat_map(|s| &s.managed).chain(confirmed) {
            managed.change.validate(&managed.source).map_err(error)?;
            if managed.destination != managed.change.destination(&managed.source)
                || managed.revision.len() != 64
                || !managed.revision.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(EngineError::Other("invalid confirmed cloud management".into()));
            }
            if !self.cloud.captures.is_empty()
                || !self.cloud.binding.as_ref().is_some_and(|b| b.destination == managed.source && b.revision == managed.previous)
            {
                continue;
            }
            match &managed.change {
                Change::Rename { file } => {
                    let old = self.saved.name.clone();
                    let name = file.rsplit('/').next().unwrap_or(file);
                    if self.doc.name == old {
                        crate::cmd::rename_design(self, &old, name);
                    }
                    Arc::make_mut(&mut self.saved).name = name.into();
                    self.cloud.binding = Some(Binding { destination: managed.destination.clone(), revision: Some(managed.revision.clone()) });
                }
                Change::Resolve { keep: solidcraft_cloud::conflicts::Keep::Local, .. } => {
                    self.cloud.binding = Some(Binding { destination: managed.destination.clone(), revision: Some(managed.revision.clone()) });
                }
                Change::Delete | Change::Resolve { keep: solidcraft_cloud::conflicts::Keep::Remote, .. } => {
                    self.cloud.binding = None;
                    self.mark_unsaved();
                }
                // Restore keeps existing tabs and edits intact. The UI can open the restored
                // head separately; older tabs retain their Outdated indication.
                Change::Restore { .. } => {}
            }
        }
        Ok(())
    }
}
