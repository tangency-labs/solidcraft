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

/// A bounded snapshot of the host's real open views. Recent IDs are most-recent-first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewContext {
    open: Vec<ViewDescriptor>,
    focused: u64,
    recent: Vec<u64>,
}

impl Default for ViewContext {
    fn default() -> Self {
        Self { open: vec![ViewDescriptor { id: 1, kind: ViewKind::Model, window: 0, subject: None }], focused: 1, recent: vec![1] }
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
        Ok(Self { open, focused, recent })
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
