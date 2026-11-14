//! The Layout page's "Design tabs" choices, read from one place by the tab strip and the views code.

use crate::prefs::Prefs;

/// How a design's views are shown and opened, copied from the preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesignTabPrefs {
    /// Views are sub-tabs inside their design's tab (off: one tab with a view switcher).
    pub views_as_tabs: bool,
    /// A drawing sheet opens as a view of its design.
    pub sub_tab_sheets: bool,
    /// Editing a sketch opens it as a view of its design.
    pub sub_tab_sketches: bool,
    /// Render and Animate open as views of their design.
    pub sub_tab_render: bool,
    /// Opening a sheet, sketch or workspace a view already shows activates that view.
    pub reuse_views: bool,
    /// A design opens with the views it had when it was last closed.
    pub reopen_views: bool,
}

impl From<&Prefs> for DesignTabPrefs {
    fn from(p: &Prefs) -> Self {
        Self {
            views_as_tabs: p.design_views_as_tabs,
            sub_tab_sheets: p.design_sub_tab_sheets,
            sub_tab_sketches: p.design_sub_tab_sketches,
            sub_tab_render: p.design_sub_tab_render,
            reuse_views: p.design_reuse_views,
            reopen_views: p.design_reopen_views,
        }
    }
}
