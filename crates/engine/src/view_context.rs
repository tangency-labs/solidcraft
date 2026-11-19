//! Validated runtime view identities supplied by a host. No UI or window toolkit state.

use solidcraft_views::{MAX_POPOUTS, MAX_VIEWS, ViewKind};

const MAX_ID: u64 = 1 << 53;
const MAX_SUBJECT: usize = 256;

/// One open view of the design. Its stable identity survives moves between windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewDescriptor {
    /// Stable view identity within this design.
    pub id: u64,
    /// Workspace this view displays.
    pub kind: ViewKind,
    /// Owning window; zero denotes the main window.
    pub window: u64,
    /// Sheet or other subject displayed by this view, when applicable.
    pub subject: Option<String>,
}

impl crate::Session {
    /// The stable UI owner of the one edit, or none for a headless sheet edit.
    pub fn editing_view(&self) -> Option<u64> {
        self.active_sketch.and(self.sketching_view).or_else(|| self.active_sheet_sketch.as_ref().and_then(|entry| entry.view))
    }

    pub(crate) fn sheet_view_matches(context: &ViewContext, document: &solidcraft_doc::Document, view: u64, drawing: u64, sheet: u64) -> bool {
        let drawing_id = solidcraft_doc::ids::encode(drawing);
        let sheet_id = solidcraft_doc::ids::encode(sheet);
        let canonical = format!("{drawing_id}/{sheet_id}");
        context.descriptors().iter().any(|descriptor| {
            descriptor.id == view
                && descriptor.kind == ViewKind::Sheet
                && descriptor.subject.as_deref().is_some_and(|subject| {
                    subject == canonical
                        || (subject == sheet_id
                            && document.drawings.iter().filter(|drawing| drawing.sheets.iter().any(|record| record.id == sheet)).count() == 1)
                })
        })
    }

    /// Resolve a Sheet tile without manufacturing host identities. Default headless sessions
    /// permit an omitted view and keep no tile owner. Hosted sessions require an open match.
    pub(crate) fn requested_sheet_view(&self, drawing: u64, sheet: u64, requested: Option<u64>, command: &str) -> crate::Result<Option<u64>> {
        let bad = |message: String| crate::EngineError::BadParams { cmd: command.into(), msg: message };
        if let Some(view) = requested {
            if !(1..=MAX_ID).contains(&view) || !Self::sheet_view_matches(&self.view_context, &self.doc, view, drawing, sheet) {
                return Err(bad(format!("view {view} must be an open Sheet view of this drawing and sheet")));
            }
            return Ok(Some(view));
        }
        if !self.view_context.is_hosted() {
            return Ok(None);
        }
        std::iter::once(self.view_context.focused())
            .chain(self.view_context.recent().iter().copied())
            .chain(self.view_context.descriptors().iter().map(|descriptor| descriptor.id))
            .find(|view| Self::sheet_view_matches(&self.view_context, &self.doc, *view, drawing, sheet))
            .map(Some)
            .ok_or_else(|| bad("no Sheet view of this drawing and sheet is open".into()))
    }

    pub(crate) fn checked_sheet_sketch_owner(
        &self,
        context: &ViewContext,
        entry: Option<&crate::SheetSketchEdit>,
        document: &solidcraft_doc::Document,
    ) -> crate::Result<()> {
        let Some(entry) = entry else { return Ok(()) };
        let valid_record = document.drawing(entry.drawing).is_some_and(|drawing| {
            drawing.sketches.iter().filter(|record| record.id == entry.sketch).count() == 1
                && drawing.sheet_sketch(entry.sketch).is_some_and(|record| record.owner == entry.owner)
        });
        if !valid_record {
            return Err(crate::EngineError::Other("the edited sheet sketch changed owner or no longer exists".into()));
        }
        let sheet = entry.sheet(document).ok_or_else(|| crate::EngineError::Other("the edited sheet sketch's owner no longer exists".into()))?;
        if entry.view.is_some_and(|view| Self::sheet_view_matches(context, document, view, entry.drawing, sheet))
            || (entry.view.is_none() && !context.is_hosted())
        {
            Ok(())
        } else {
            Err(crate::EngineError::Other("the sheet sketch's editing view is closed or displays another sheet".into()))
        }
    }
}

/// A bounded snapshot of the host's real open views. Recent IDs are most-recent-first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewContext {
    open: Vec<ViewDescriptor>,
    focused: u64,
    recent: Vec<u64>,
    hosted: bool,
}

impl Default for ViewContext {
    fn default() -> Self {
        Self { open: vec![ViewDescriptor { id: 1, kind: ViewKind::Model, window: 0, subject: None }], focused: 1, recent: vec![1], hosted: false }
    }
}

impl ViewContext {
    /// Validate the host snapshot, including identities, membership, window count and size caps.
    pub fn new(open: Vec<ViewDescriptor>, focused: u64, recent: Vec<u64>) -> Result<Self, String> {
        if open.is_empty() || open.len() > MAX_VIEWS || recent.len() > MAX_VIEWS {
            return Err("view context must contain 1 to 64 open views and at most 64 recent IDs".into());
        }
        let mut ids = Vec::new();
        let mut windows = Vec::new();
        for view in &open {
            if !(1..=MAX_ID).contains(&view.id) || ids.contains(&view.id) {
                return Err("view context contains an invalid or duplicate view ID".into());
            }
            if view.window > u64::from(u32::MAX) || view.subject.as_ref().is_some_and(|subject| subject.len() > MAX_SUBJECT) {
                return Err("view context contains an invalid window or subject".into());
            }
            ids.push(view.id);
            if !windows.contains(&view.window) {
                windows.push(view.window);
            }
        }
        if windows.iter().filter(|window| **window != 0).count() > MAX_POPOUTS || !ids.contains(&focused) {
            return Err("view context has too many windows or its focused view is closed".into());
        }
        let mut seen = Vec::new();
        for id in &recent {
            if !ids.contains(id) || seen.contains(id) {
                return Err("view context recent IDs must be unique open views".into());
            }
            seen.push(*id);
        }
        Ok(Self { open, focused, recent, hosted: true })
    }

    /// Whether a host supplied real view identities. Headless sessions own no Sheet tile.
    pub fn is_hosted(&self) -> bool {
        self.hosted
    }

    /// All open view identities supplied by the host.
    pub fn descriptors(&self) -> &[ViewDescriptor] {
        &self.open
    }
    /// The host's focused open view.
    pub fn focused(&self) -> u64 {
        self.focused
    }
    /// Focus history, most-recent-first, containing only unique open view IDs.
    pub fn recent(&self) -> &[u64] {
        &self.recent
    }

    /// Resolve an explicit target, otherwise the focused Model then the most recently focused
    /// Model. The host must open a real Model when none exists; this never invents identities.
    pub fn resolve_sketch_target(&self, requested: Option<u64>) -> Result<u64, String> {
        if let Some(id) = requested {
            let view = self.open.iter().find(|view| view.id == id).ok_or_else(|| format!("no open view {id}"))?;
            return if view.kind == ViewKind::Model { Ok(id) } else { Err(format!("view {id} cannot edit a model sketch: use a Model view")) };
        }
        let model = |id| self.open.iter().find(|view| view.id == id && view.kind == ViewKind::Model).map(|view| view.id);
        model(self.focused)
            .or_else(|| self.recent.iter().find_map(|id| model(*id)))
            .or_else(|| self.open.iter().find(|view| view.kind == ViewKind::Model).map(|view| view.id))
            .ok_or_else(|| "no Model view is open; open a Model view before starting a sketch".into())
    }
}
