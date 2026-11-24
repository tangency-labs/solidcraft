//! Plugin state the app keeps between frames ([`crate::SolveApp::plugins`]).
//!
//! New plugin state goes here rather than on `SolveApp`: one field, in alphabetical order, of a
//! type with a `Default`. That way it needs no line in `SolveApp::new`, and plugin PRs don't all
//! add their fields at the same spot.

/// Plugin state the app keeps between frames.
#[derive(Default)]
pub(crate) struct State {
    /// Design-change notifications for plugins ([`crate::plugin_events`]).
    pub(crate) events: crate::plugin_events::State,
    /// Set by the desktop app ([`crate::SolveApp::share_plugin_grants`]): the approved plugin grants
    /// are kept in the plugins folder's grant store too, shared with the command line and headless MCP.
    pub(crate) grant_store: Option<solidcraft_engine::plugin::grant_store::Synced>,
    /// Most recent completed inspection per session and package, ordered by request.
    pub(crate) inspection_versions: std::collections::BTreeMap<(u64, String), usize>,
}
