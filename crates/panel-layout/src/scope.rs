//! The owner and window-local scope a host's panels are bound to. Plain ids: no toolkit state.
use crate::model::{DocumentId, PanelResult, ViewportId, WorkspaceId, valid_id};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalScope {
    pub workspace: WorkspaceId,
    pub document: Option<DocumentId>,
    pub viewport: ViewportId,
}
impl LocalScope {
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.workspace.0)?;
        valid_id(&self.viewport.0)?;
        if let Some(id) = &self.document {
            valid_id(&id.0)?;
        }
        Ok(())
    }
}
